//! Capability-aware source module resolution and lifecycle.

use std::{
    collections::BTreeMap,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    thread::ThreadId,
};

use sim_kernel::{CapabilityName, Cx, Dir, Error, Event, Expr, Result, Symbol};
use sim_lib_binding::BindingCell;
use sim_lib_core::{
    ReadEvalBroker, ReadEvalDecision, ReadEvalRequest, ReadEvalSource, RequestOrigin,
    SourceAuthority,
};
use sim_shape::AnyShape;

use crate::{IdentitySpecifierPolicy, ModuleSpecifierPolicy, SpecifierPolicyRequest};

/// Capability required before namespace source resolution begins.
pub fn module_load_capability() -> CapabilityName {
    CapabilityName::new("namespace.module.load")
}

/// Canonical identity of a module within one caller-named root.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleIdentity {
    root: Symbol,
    path: String,
}

impl ModuleIdentity {
    /// Caller-supplied root identity.
    pub fn root(&self) -> &Symbol {
        &self.root
    }
    /// Normalized, root-relative module path.
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// Complete input for resolving one source module.
pub struct ModuleRequest {
    /// Stable identity assigned to the supplied root by its caller.
    pub root_id: Symbol,
    /// The only directory through which source may be resolved.
    pub root: Arc<dyn Dir>,
    /// Importing module identity for relative resolution, if any.
    pub importer: Option<ModuleIdentity>,
    /// Root-relative or `./` / `../` module specifier.
    pub specifier: String,
    /// Installed codec used by the read-eval broker.
    pub codec: Symbol,
    /// Trusted host authority governing module source and evaluation.
    pub authority: SourceAuthority,
}

impl ModuleRequest {
    /// Builds a module request with every source and authority input explicit.
    pub fn new(
        root_id: Symbol,
        root: Arc<dyn Dir>,
        importer: Option<ModuleIdentity>,
        specifier: String,
        codec: Symbol,
        authority: SourceAuthority,
    ) -> Self {
        Self {
            root_id,
            root,
            importer,
            specifier,
            codec,
            authority,
        }
    }
}

/// A linked module and its stable live default-export edge.
#[derive(Clone, Debug)]
pub struct ModuleInstance {
    identity: ModuleIdentity,
    generation: u64,
    default_export: BindingCell,
}

impl ModuleInstance {
    /// Canonical module identity.
    pub fn identity(&self) -> &ModuleIdentity {
        &self.identity
    }
    /// Successful replacement generation, starting at one.
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Live binding followed by importers across cache replacement.
    pub fn default_export(&self) -> &BindingCell {
        &self.default_export
    }
}

/// Inspectable terminal outcome for a resolution attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleResolutionOutcome {
    /// Source was decoded, evaluated, and linked.
    Linked,
    /// An already linked cache generation was returned.
    CacheHit,
    /// Resolution was refused before read-eval was reached.
    ReadRefused,
    /// The selected codec could not decode the source.
    DecodeFailed,
    /// Evaluation or its result-shape check failed.
    EvalFailed,
    /// The initializing thread requested the same canonical module again.
    Cycle,
}

/// Deterministic evidence published for every terminal resolution attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleResolutionReceipt {
    /// Canonical requested identity.
    pub identity: ModuleIdentity,
    /// Cache generation observed or produced.
    pub generation: u64,
    /// Terminal result.
    pub outcome: ModuleResolutionOutcome,
    /// Stable failure text when resolution did not link.
    pub detail: Option<String>,
    /// Exact existing read-eval ledger event, when evaluation was reached.
    pub read_eval_event: Option<Event>,
}

enum CacheState {
    Initializing {
        owner: ThreadId,
        generation: u64,
    },
    Linked(ModuleInstance),
    Failed {
        generation: u64,
        message: String,
        binding: BindingCell,
        outcome: ModuleResolutionOutcome,
    },
}

#[derive(Default)]
struct LoaderState {
    cache: BTreeMap<ModuleIdentity, CacheState>,
    receipts: Vec<ModuleResolutionReceipt>,
}

/// Source-bound module cache. No loader lock is held during storage or user evaluation.
pub struct ModuleLoader {
    state: Mutex<LoaderState>,
    changed: Condvar,
    broker: ReadEvalBroker,
    specifier_policy: Arc<dyn ModuleSpecifierPolicy>,
}

impl Default for ModuleLoader {
    fn default() -> Self {
        Self {
            state: Mutex::new(LoaderState::default()),
            changed: Condvar::new(),
            broker: ReadEvalBroker::default(),
            specifier_policy: Arc::new(IdentitySpecifierPolicy),
        }
    }
}

impl ModuleLoader {
    /// Creates an empty loader and receipt history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a loader with an explicit bounded textual specifier policy.
    pub fn with_specifier_policy(specifier_policy: Arc<dyn ModuleSpecifierPolicy>) -> Self {
        Self {
            specifier_policy,
            ..Self::default()
        }
    }

    /// Resolves and links a module, sharing concurrent work and cached failures.
    pub fn load(&self, cx: &mut Cx, request: ModuleRequest) -> Result<ModuleInstance> {
        cx.require(&module_load_capability())?;
        let identity = canonical_identity(&request, self.specifier_policy.as_ref())?;
        let owner = std::thread::current().id();
        let (generation, binding) = loop {
            let mut state = self.lock_state()?;
            match state.cache.get(&identity) {
                Some(CacheState::Linked(instance)) => {
                    let instance = instance.clone();
                    push_receipt(
                        &mut state,
                        &identity,
                        instance.generation,
                        ModuleResolutionOutcome::CacheHit,
                        None,
                        None,
                    );
                    return Ok(instance);
                }
                Some(CacheState::Failed {
                    generation,
                    message,
                    outcome,
                    ..
                }) => {
                    let generation = *generation;
                    let message = message.clone();
                    let outcome = *outcome;
                    push_receipt(
                        &mut state,
                        &identity,
                        generation,
                        outcome,
                        Some(message.clone()),
                        None,
                    );
                    return Err(Error::Eval(message));
                }
                Some(CacheState::Initializing {
                    owner: active,
                    generation,
                    ..
                }) if *active == owner => {
                    let generation = *generation;
                    let message = format!("module cycle at {}:{}", identity.root, identity.path);
                    push_receipt(
                        &mut state,
                        &identity,
                        generation,
                        ModuleResolutionOutcome::Cycle,
                        Some(message.clone()),
                        None,
                    );
                    return Err(Error::Eval(message));
                }
                Some(CacheState::Initializing { .. }) => {
                    drop(
                        self.changed
                            .wait(state)
                            .map_err(|_| Error::PoisonedLock("module loader"))?,
                    );
                    continue;
                }
                None => {
                    let binding = BindingCell::uninitialized(Symbol::new(identity.path.clone()));
                    state.cache.insert(
                        identity.clone(),
                        CacheState::Initializing {
                            owner,
                            generation: 1,
                        },
                    );
                    break (1, binding);
                }
            }
        };
        self.finish_load(cx, request, identity, generation, binding)
    }

    /// Forces a replacement load while preserving existing live bindings.
    pub fn reload(&self, cx: &mut Cx, request: ModuleRequest) -> Result<ModuleInstance> {
        cx.require(&module_load_capability())?;
        let identity = canonical_identity(&request, self.specifier_policy.as_ref())?;
        let owner = std::thread::current().id();
        let (generation, binding) = {
            let mut state = self.lock_state()?;
            let (generation, binding) = match state.cache.remove(&identity) {
                Some(CacheState::Linked(instance)) => {
                    (instance.generation + 1, instance.default_export)
                }
                Some(CacheState::Failed {
                    generation,
                    binding,
                    ..
                }) => (generation + 1, binding),
                Some(initializing @ CacheState::Initializing { .. }) => {
                    state.cache.insert(identity.clone(), initializing);
                    return Err(Error::Eval(format!(
                        "cannot replace initializing module {}:{}",
                        identity.root, identity.path
                    )));
                }
                None => (
                    1,
                    BindingCell::uninitialized(Symbol::new(identity.path.clone())),
                ),
            };
            state.cache.insert(
                identity.clone(),
                CacheState::Initializing { owner, generation },
            );
            (generation, binding)
        };
        self.finish_load(cx, request, identity, generation, binding)
    }

    /// Snapshot of ordered resolution evidence.
    pub fn receipts(&self) -> Result<Vec<ModuleResolutionReceipt>> {
        Ok(self.lock_state()?.receipts.clone())
    }

    /// Snapshot of the diminished read-eval decisions linked to module loads.
    pub fn decisions(&self, cx: &Cx) -> Result<Vec<ReadEvalDecision>> {
        self.broker.decisions(cx)
    }

    fn finish_load(
        &self,
        cx: &mut Cx,
        request: ModuleRequest,
        identity: ModuleIdentity,
        generation: u64,
        binding: BindingCell,
    ) -> Result<ModuleInstance> {
        let admission = match read_source(cx, request.root.as_ref(), &identity.path) {
            Ok(source) => Some(self.broker.admit_with_event(
                cx,
                ReadEvalRequest::new(
                    RequestOrigin::with_detail(
                        Symbol::qualified("namespace", "module"),
                        Expr::String(format!("{}:{}", identity.root, identity.path)),
                    ),
                    request.codec,
                    source,
                    request.authority,
                    Arc::new(AnyShape),
                ),
            )?),
            Err(error) => {
                return self.finish_refused(identity, generation, binding, error);
            }
        };
        let admission = admission.expect("successful read creates an admission");
        let outcome = match admission.decision.outcome {
            sim_lib_core::ReadEvalOutcome::DecodeFailed => ModuleResolutionOutcome::DecodeFailed,
            sim_lib_core::ReadEvalOutcome::Admitted => ModuleResolutionOutcome::Linked,
            _ => ModuleResolutionOutcome::EvalFailed,
        };
        let event = admission.event;
        let result = admission.result;
        let mut state = self.lock_state()?;
        match result {
            Ok(value) => {
                binding.set(value)?;
                let instance = ModuleInstance {
                    identity: identity.clone(),
                    generation,
                    default_export: binding,
                };
                state
                    .cache
                    .insert(identity.clone(), CacheState::Linked(instance.clone()));
                push_receipt(
                    &mut state,
                    &identity,
                    generation,
                    ModuleResolutionOutcome::Linked,
                    None,
                    Some(event),
                );
                self.changed.notify_all();
                Ok(instance)
            }
            Err(error) => {
                let message = error.to_string();
                state.cache.insert(
                    identity.clone(),
                    CacheState::Failed {
                        generation,
                        message: message.clone(),
                        binding,
                        outcome,
                    },
                );
                push_receipt(
                    &mut state,
                    &identity,
                    generation,
                    outcome,
                    Some(message.clone()),
                    Some(event),
                );
                self.changed.notify_all();
                Err(Error::Eval(message))
            }
        }
    }

    fn finish_refused(
        &self,
        identity: ModuleIdentity,
        generation: u64,
        binding: BindingCell,
        error: Error,
    ) -> Result<ModuleInstance> {
        let message = error.to_string();
        let mut state = self.lock_state()?;
        state.cache.insert(
            identity.clone(),
            CacheState::Failed {
                generation,
                message: message.clone(),
                binding,
                outcome: ModuleResolutionOutcome::ReadRefused,
            },
        );
        push_receipt(
            &mut state,
            &identity,
            generation,
            ModuleResolutionOutcome::ReadRefused,
            Some(message.clone()),
            None,
        );
        self.changed.notify_all();
        Err(Error::Eval(message))
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, LoaderState>> {
        self.state
            .lock()
            .map_err(|_| Error::PoisonedLock("module loader"))
    }
}

/// Codec-bound source-module policy shared by guest-language adapters.
///
/// The policy owns exactly one loader, and therefore one cache and ordered
/// evidence stream. Roots and authority remain request inputs: this shared
/// type deliberately supplies neither an ambient root nor guest defaults.
pub struct SourceModulePolicy {
    loader: ModuleLoader,
    codec: Symbol,
}

impl SourceModulePolicy {
    /// Creates a policy for an installed codec and explicit specifier policy.
    pub fn new(codec: Symbol, specifier_policy: Arc<dyn ModuleSpecifierPolicy>) -> Self {
        Self {
            loader: ModuleLoader::with_specifier_policy(specifier_policy),
            codec,
        }
    }

    /// Loads a root-relative module through the caller-supplied root and authority.
    pub fn load(
        &self,
        cx: &mut Cx,
        root_id: Symbol,
        root: Arc<dyn Dir>,
        specifier: impl Into<String>,
        authority: SourceAuthority,
    ) -> Result<ModuleInstance> {
        self.load_from(cx, root_id, root, None, specifier, authority)
    }

    /// Loads a module, optionally resolving it relative to an importer.
    pub fn load_from(
        &self,
        cx: &mut Cx,
        root_id: Symbol,
        root: Arc<dyn Dir>,
        importer: Option<ModuleIdentity>,
        specifier: impl Into<String>,
        authority: SourceAuthority,
    ) -> Result<ModuleInstance> {
        self.loader.load(
            cx,
            ModuleRequest::new(
                root_id,
                root,
                importer,
                specifier.into(),
                self.codec.clone(),
                authority,
            ),
        )
    }

    /// Dynamically imports through the same cache and authority boundary as static loads.
    pub fn dynamic_import(
        &self,
        cx: &mut Cx,
        root_id: Symbol,
        root: Arc<dyn Dir>,
        importer: Option<ModuleIdentity>,
        specifier: impl Into<String>,
        authority: SourceAuthority,
    ) -> Result<ModuleInstance> {
        self.load_from(cx, root_id, root, importer, specifier, authority)
    }

    /// Re-evaluates a module while preserving its existing live binding edge.
    pub fn reload(
        &self,
        cx: &mut Cx,
        root_id: Symbol,
        root: Arc<dyn Dir>,
        importer: Option<ModuleIdentity>,
        specifier: impl Into<String>,
        authority: SourceAuthority,
    ) -> Result<ModuleInstance> {
        self.loader.reload(
            cx,
            ModuleRequest::new(
                root_id,
                root,
                importer,
                specifier.into(),
                self.codec.clone(),
                authority,
            ),
        )
    }

    /// Snapshot of ordered module lifecycle receipts.
    pub fn receipts(&self) -> Result<Vec<ModuleResolutionReceipt>> {
        self.loader.receipts()
    }

    /// Snapshot of the linked diminished read-eval decisions.
    pub fn decisions(&self, cx: &Cx) -> Result<Vec<ReadEvalDecision>> {
        self.loader.decisions(cx)
    }
}

fn canonical_identity(
    request: &ModuleRequest,
    policy: &dyn ModuleSpecifierPolicy,
) -> Result<ModuleIdentity> {
    let policy_request =
        SpecifierPolicyRequest::new(request.importer.clone(), vec![request.specifier.clone()])
            .map_err(|refusal| Error::Eval(refusal.to_string()))?;
    let specifier = policy
        .resolve(&policy_request)
        .map_err(|refusal| Error::Eval(refusal.to_string()))?;
    let specifier = specifier.as_str();
    let absolute = specifier.starts_with('/');
    if absolute {
        return Err(Error::Eval(
            "module specifier must be root-relative, not absolute".to_owned(),
        ));
    }
    let mut parts = if specifier.starts_with('.') {
        let importer = request
            .importer
            .as_ref()
            .ok_or_else(|| Error::Eval("relative module request has no importer".to_owned()))?;
        if importer.root != request.root_id {
            return Err(Error::Eval(
                "relative module request crosses supplied roots".to_owned(),
            ));
        }
        let mut base: Vec<&str> = importer.path.split('/').collect();
        base.pop();
        base
    } else {
        Vec::new()
    };
    for part in specifier.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(Error::Eval(
                        "module request escapes supplied root".to_owned(),
                    ));
                }
            }
            component if component.contains('\\') => {
                return Err(Error::Eval(
                    "module path contains a non-canonical separator".to_owned(),
                ));
            }
            component => parts.push(component),
        }
    }
    if parts.is_empty() {
        return Err(Error::Eval("module path is empty".to_owned()));
    }
    Ok(ModuleIdentity {
        root: request.root_id.clone(),
        path: parts.join("/"),
    })
}

fn read_source(cx: &mut Cx, root: &dyn Dir, path: &str) -> Result<ReadEvalSource> {
    let components = path.split('/').collect::<Vec<_>>();
    read_source_at(cx, root, &components, path)
}

fn read_source_at(
    cx: &mut Cx,
    dir: &dyn Dir,
    components: &[&str],
    path: &str,
) -> Result<ReadEvalSource> {
    let (component, rest) = components
        .split_first()
        .ok_or_else(|| Error::Eval("module path is empty".to_owned()))?;
    let key = Symbol::new(*component);
    if rest.is_empty() {
        if !dir.has(cx, key.clone())? {
            return Err(Error::Eval(format!("module source not found: {path}")));
        }
        let value = dir.get(cx, key)?;
        return match value.object().as_expr(cx)? {
            Expr::String(text) => Ok(ReadEvalSource::Text(text)),
            Expr::Bytes(bytes) => Ok(ReadEvalSource::Bytes(bytes)),
            _ => Err(Error::Eval(format!(
                "module source is not text or bytes: {path}"
            ))),
        };
    }
    let value = dir
        .opendir(cx, key)?
        .ok_or_else(|| Error::Eval(format!("module directory not found: {path}")))?;
    let child = value
        .object()
        .as_dir()
        .ok_or_else(|| Error::Eval(format!("module path component is not a Dir: {component}")))?;
    read_source_at(cx, child, rest, path)
}

fn push_receipt(
    state: &mut LoaderState,
    identity: &ModuleIdentity,
    generation: u64,
    outcome: ModuleResolutionOutcome,
    detail: Option<String>,
    read_eval_event: Option<Event>,
) {
    state.receipts.push(ModuleResolutionReceipt {
        identity: identity.clone(),
        generation,
        outcome,
        detail,
        read_eval_event,
    });
}

#[cfg(test)]
mod tests;
