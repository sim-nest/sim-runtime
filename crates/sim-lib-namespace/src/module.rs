//! Capability-aware source module resolution and lifecycle.

use std::{
    collections::BTreeMap,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    thread::ThreadId,
};

use sim_kernel::{CapabilityName, CapabilitySet, Cx, Dir, Error, Expr, ReadPolicy, Result, Symbol};
use sim_lib_binding::BindingCell;
use sim_lib_core::{ReadEvalBroker, ReadEvalRequest, ReadEvalSource, RequestOrigin};
use sim_shape::AnyShape;

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
    /// Trusted host-built read policy.
    pub read_policy: ReadPolicy,
    /// Additional caller powers required by this module.
    pub requires: Vec<CapabilityName>,
    /// Diminished powers under which module code evaluates.
    pub allow: CapabilitySet,
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
    /// Resolution, decoding, or evaluation failed.
    Failed,
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
    },
}

#[derive(Default)]
struct LoaderState {
    cache: BTreeMap<ModuleIdentity, CacheState>,
    receipts: Vec<ModuleResolutionReceipt>,
}

/// Source-bound module cache. No loader lock is held during storage or user evaluation.
#[derive(Default)]
pub struct ModuleLoader {
    state: Mutex<LoaderState>,
    changed: Condvar,
    broker: ReadEvalBroker,
}

impl ModuleLoader {
    /// Creates an empty loader and receipt history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves and links a module, sharing concurrent work and cached failures.
    pub fn load(&self, cx: &mut Cx, request: ModuleRequest) -> Result<ModuleInstance> {
        cx.require(&module_load_capability())?;
        let identity = canonical_identity(&request)?;
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
                    );
                    return Ok(instance);
                }
                Some(CacheState::Failed {
                    generation,
                    message,
                    ..
                }) => {
                    let generation = *generation;
                    let message = message.clone();
                    push_receipt(
                        &mut state,
                        &identity,
                        generation,
                        ModuleResolutionOutcome::Failed,
                        Some(message.clone()),
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
        let identity = canonical_identity(&request)?;
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

    fn finish_load(
        &self,
        cx: &mut Cx,
        request: ModuleRequest,
        identity: ModuleIdentity,
        generation: u64,
        binding: BindingCell,
    ) -> Result<ModuleInstance> {
        let result = (|| {
            let source = read_source(cx, request.root.as_ref(), &identity.path)?;
            self.broker.admit(
                cx,
                ReadEvalRequest {
                    origin: RequestOrigin::with_detail(
                        Symbol::qualified("namespace", "module"),
                        Expr::String(format!("{}:{}", identity.root, identity.path)),
                    ),
                    codec: request.codec,
                    source,
                    read_policy: request.read_policy,
                    requires: request.requires,
                    allow: request.allow,
                    expected_shape: Arc::new(AnyShape),
                },
            )
        })();
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
                    },
                );
                push_receipt(
                    &mut state,
                    &identity,
                    generation,
                    ModuleResolutionOutcome::Failed,
                    Some(message.clone()),
                );
                self.changed.notify_all();
                Err(Error::Eval(message))
            }
        }
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, LoaderState>> {
        self.state
            .lock()
            .map_err(|_| Error::PoisonedLock("module loader"))
    }
}

fn canonical_identity(request: &ModuleRequest) -> Result<ModuleIdentity> {
    let absolute = request.specifier.starts_with('/');
    if absolute {
        return Err(Error::Eval(
            "module specifier must be root-relative, not absolute".to_owned(),
        ));
    }
    let mut parts = if request.specifier.starts_with('.') {
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
    for part in request.specifier.split('/') {
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
) {
    state.receipts.push(ModuleResolutionReceipt {
        identity: identity.clone(),
        generation,
        outcome,
        detail,
    });
}

#[cfg(test)]
mod tests;
