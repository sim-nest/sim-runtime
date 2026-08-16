//! Resumable JVMS class-initialization sequencing.

use std::sync::Arc;

use sim_lib_control::Raised;
use sim_lib_mutation::{ManagedHandle, RootedHandle};

use crate::{JvmEdge, JvmGraphError, JvmHeap, JvmRole};

/// Stable identity of the execution lane that owns an initialization attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InitializationLane(pub u64);

/// One deterministic operation in a class-initialization plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InitializationAction {
    /// Assign the prepared `ConstantValue` fields of this class.
    InstallStaticConstants,
    /// Initialize the direct superclass.
    InitializeSuperclass(Arc<str>),
    /// Initialize a superinterface that declares a default method.
    InitializeSuperinterface(Arc<str>),
    /// Invoke this class's `<clinit>` method.
    InvokeClassInitializer,
}

/// Bounded, immutable ordering inputs for one class.
#[derive(Clone, Debug)]
pub struct InitializationPlan {
    class: Arc<str>,
    actions: Vec<InitializationAction>,
}

impl InitializationPlan {
    /// Creates a plan in exact JVMS order. `default_method_interfaces` must be
    /// supplied in recursive enumeration order, with duplicates removed.
    pub fn new(
        class: impl Into<Arc<str>>,
        superclass: Option<impl Into<Arc<str>>>,
        default_method_interfaces: impl IntoIterator<Item = impl Into<Arc<str>>>,
        has_class_initializer: bool,
        max_actions: usize,
    ) -> Result<Self, InitializationError> {
        let mut actions = vec![InitializationAction::InstallStaticConstants];
        if let Some(superclass) = superclass {
            actions.push(InitializationAction::InitializeSuperclass(
                superclass.into(),
            ));
        }
        actions.extend(
            default_method_interfaces
                .into_iter()
                .map(|name| InitializationAction::InitializeSuperinterface(name.into())),
        );
        if has_class_initializer {
            actions.push(InitializationAction::InvokeClassInitializer);
        }
        if actions.len() > max_actions {
            return Err(InitializationError::PlanLimit {
                required: actions.len(),
                limit: max_actions,
            });
        }
        Ok(Self {
            class: class.into(),
            actions,
        })
    }

    /// Class whose initialization is described.
    pub fn class(&self) -> &str {
        &self.class
    }
}

/// Durable state visible to diagnostics and scheduling policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassInitializationState {
    /// No active use has triggered initialization.
    Uninitialized,
    /// One lane owns initialization at the reported action cursor.
    Initializing {
        /// Lane permitted to advance this attempt.
        lane: InitializationLane,
        /// Next immutable plan action to issue.
        cursor: usize,
    },
    /// Initialization completed normally.
    Initialized,
    /// Initialization completed abruptly and can never be retried.
    Erroneous,
}

/// A bounded browse projection that does not expose recursive throwable data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitializationSnapshot {
    /// Binary class name.
    pub class: Arc<str>,
    /// Current lifecycle state.
    pub state: ClassInitializationState,
    /// Total number of actions in the immutable plan.
    pub action_count: usize,
}

/// Result of requesting progress from an initialization lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InitializationResume {
    /// Caller must perform this action and report its completion.
    Action(InitializationAction),
    /// A recursive request by the owning lane may proceed without blocking.
    Reentrant,
    /// Initialization is complete.
    Initialized,
    /// Initialization is permanently erroneous; this is the stored wrapper.
    Erroneous(Raised),
}

/// Failure at the bounded initialization boundary.
#[derive(Debug)]
pub enum InitializationError {
    /// The immutable action plan exceeds its declared bound.
    PlanLimit {
        /// Number of actions required by the supplied hierarchy.
        required: usize,
        /// Maximum actions admitted by the caller.
        limit: usize,
    },
    /// A different lane attempted to advance an initialization it does not own.
    LaneBusy,
    /// No action is awaiting completion.
    NoPendingAction,
    /// The class has already reached a terminal state.
    AlreadyComplete,
    /// Managed allocation or mutation failed.
    Graph(JvmGraphError),
}

impl From<JvmGraphError> for InitializationError {
    fn from(value: JvmGraphError) -> Self {
        Self::Graph(value)
    }
}

/// Managed, resumable initialization state for one class mirror.
pub struct ClassInitialization {
    plan: InitializationPlan,
    class_mirror: ManagedHandle,
    state: ClassInitializationState,
    pending: bool,
    failure: Option<Raised>,
    failure_root: Option<RootedHandle>,
}

impl ClassInitialization {
    /// Attaches a fresh state machine to its already-managed class mirror.
    pub fn new(plan: InitializationPlan, class_mirror: ManagedHandle) -> Self {
        Self {
            plan,
            class_mirror,
            state: ClassInitializationState::Uninitialized,
            pending: false,
            failure: None,
            failure_root: None,
        }
    }

    /// Returns a deterministic, bounded browse projection.
    pub fn snapshot(&self) -> InitializationSnapshot {
        InitializationSnapshot {
            class: self.plan.class.clone(),
            state: self.state,
            action_count: self.plan.actions.len(),
        }
    }

    /// Triggers or resumes initialization. An interruption after this method
    /// returns an action is harmless: the cursor advances only through
    /// [`Self::complete_action`].
    pub fn resume(
        &mut self,
        lane: InitializationLane,
    ) -> Result<InitializationResume, InitializationError> {
        let cursor = match self.state {
            ClassInitializationState::Uninitialized => {
                self.state = ClassInitializationState::Initializing { lane, cursor: 0 };
                0
            }
            ClassInitializationState::Initializing {
                lane: owner,
                cursor,
            } if owner == lane => {
                if self.pending {
                    return Ok(InitializationResume::Reentrant);
                }
                cursor
            }
            ClassInitializationState::Initializing { .. } => {
                return Err(InitializationError::LaneBusy);
            }
            ClassInitializationState::Initialized => {
                return Ok(InitializationResume::Initialized);
            }
            ClassInitializationState::Erroneous => {
                return Ok(InitializationResume::Erroneous(
                    self.failure
                        .clone()
                        .expect("erroneous state stores wrapper"),
                ));
            }
        };
        let Some(action) = self.plan.actions.get(cursor).cloned() else {
            self.state = ClassInitializationState::Initialized;
            return Ok(InitializationResume::Initialized);
        };
        self.pending = true;
        Ok(InitializationResume::Action(action))
    }

    /// Commits the currently pending action and advances exactly one cursor.
    pub fn complete_action(&mut self, lane: InitializationLane) -> Result<(), InitializationError> {
        let ClassInitializationState::Initializing {
            lane: owner,
            cursor,
        } = self.state
        else {
            return Err(InitializationError::AlreadyComplete);
        };
        if owner != lane {
            return Err(InitializationError::LaneBusy);
        }
        if !self.pending {
            return Err(InitializationError::NoPendingAction);
        }
        self.pending = false;
        self.state = ClassInitializationState::Initializing {
            lane,
            cursor: cursor + 1,
        };
        Ok(())
    }

    /// Permanently records abrupt completion. `wrap` constructs the specified
    /// Java initializer error in the already-landed language-neutral envelope.
    /// The managed wrapper retains the original throwable as its cause.
    pub fn fail(
        &mut self,
        heap: &mut JvmHeap,
        lane: InitializationLane,
        original: ManagedHandle,
        original_raised: &Raised,
        wrap: impl FnOnce(&Raised) -> Raised,
    ) -> Result<Raised, InitializationError> {
        match self.state {
            ClassInitializationState::Initializing { lane: owner, .. } if owner == lane => {}
            ClassInitializationState::Initializing { .. } => {
                return Err(InitializationError::LaneBusy);
            }
            _ => return Err(InitializationError::AlreadyComplete),
        }
        let wrapper = heap
            .allocate(JvmRole::Throwable)
            .map_err(JvmGraphError::from)?;
        heap.strong(wrapper, JvmEdge::Cause, original)?;
        let root = heap.root(wrapper).map_err(JvmGraphError::from)?;
        let raised = wrap(original_raised);
        self.pending = false;
        self.failure = Some(raised.clone());
        self.failure_root = Some(root);
        self.state = ClassInitializationState::Erroneous;
        Ok(raised)
    }

    /// Releases the persisted diagnostic graph when the class state itself is
    /// retired, allowing both wrapper and cause to be collected.
    pub fn release_failure(&mut self, heap: &mut JvmHeap) -> Result<(), InitializationError> {
        if let Some(root) = self.failure_root.take() {
            heap.release_root(root).map_err(JvmGraphError::from)?;
        }
        Ok(())
    }

    /// Managed class mirror owning this transition state.
    pub const fn class_mirror(&self) -> ManagedHandle {
        self.class_mirror
    }
}
