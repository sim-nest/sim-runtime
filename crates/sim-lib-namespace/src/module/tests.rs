// conformance: source module lifecycle and cross-organ language-neutral composition.

use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

use sim_codec_lisp::LispCodecLib;
use sim_kernel::{
    ClassId, ClassRef, DefaultFactory, EagerPolicy, Object, ObjectCompat, Table, TrustLevel, Value,
    read_eval_capability,
};
use sim_lib_binding::{CallArgument, CallParameter, CallSignature};
use sim_lib_control::{
    AdmissionLimit, FrameLimits, JobQueues, ResumableFrame, ResumePacket, ResumeResult, WorkLimit,
};
use sim_lib_dispatch::{DataDescriptor, Descriptor, PropertyStore};
use sim_lib_mutation::{
    EdgeId, EdgeVisitor, HardCappedRetainPolicy, ManagedArena, ManagedId, ManagedObject,
};

use super::*;

#[derive(Default)]
struct MemoryDir {
    files: RwLock<BTreeMap<Symbol, Value>>,
    dirs: RwLock<BTreeMap<Symbol, Arc<MemoryDir>>>,
}

impl MemoryDir {
    fn directory(&self, cx: &mut Cx, name: &str) -> Arc<Self> {
        let dir = Arc::new(Self::default());
        self.dirs
            .write()
            .unwrap()
            .insert(Symbol::new(name), dir.clone());
        let _ = cx;
        dir
    }

    fn source(&self, cx: &mut Cx, name: &str, source: &str) {
        self.files.write().unwrap().insert(
            Symbol::new(name),
            cx.factory().string(source.to_owned()).unwrap(),
        );
    }
}

impl Object for MemoryDir {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("memory-module-root".to_owned())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for MemoryDir {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory()
            .class_stub(ClassId(0), Symbol::qualified("test", "ModuleRoot"))
    }
    fn as_table_impl(&self) -> Option<&dyn Table> {
        Some(self)
    }
    fn as_dir(&self) -> Option<&dyn Dir> {
        Some(self)
    }
}

impl Table for MemoryDir {
    fn backend_symbol(&self) -> Symbol {
        Symbol::qualified("test", "module-root")
    }
    fn get(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
        self.files
            .read()
            .unwrap()
            .get(&key)
            .cloned()
            .map_or_else(|| cx.factory().nil(), Ok)
    }
    fn set(&self, _cx: &mut Cx, key: Symbol, value: Value) -> Result<()> {
        self.files.write().unwrap().insert(key, value);
        Ok(())
    }
    fn has(&self, _cx: &mut Cx, key: Symbol) -> Result<bool> {
        Ok(self.files.read().unwrap().contains_key(&key))
    }
    fn del(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
        self.files
            .write()
            .unwrap()
            .remove(&key)
            .map_or_else(|| cx.factory().nil(), Ok)
    }
    fn keys(&self, _cx: &mut Cx) -> Result<Vec<Symbol>> {
        Ok(self.files.read().unwrap().keys().cloned().collect())
    }
    fn entries(&self, _cx: &mut Cx) -> Result<Vec<(Symbol, Value)>> {
        Ok(self
            .files
            .read()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }
    fn len(&self, _cx: &mut Cx) -> Result<usize> {
        Ok(self.files.read().unwrap().len())
    }
    fn clear(&self, _cx: &mut Cx) -> Result<()> {
        self.files.write().unwrap().clear();
        Ok(())
    }
}

impl Dir for MemoryDir {
    fn mkdir(&self, cx: &mut Cx, name: Symbol) -> Result<Value> {
        let dir = self.directory(cx, &name.name);
        cx.factory().opaque(dir)
    }
    fn opendir(&self, cx: &mut Cx, name: Symbol) -> Result<Option<Value>> {
        self.dirs
            .read()
            .unwrap()
            .get(&name)
            .cloned()
            .map(|dir| cx.factory().opaque(dir))
            .transpose()
    }
    fn rmdir(&self, cx: &mut Cx, name: Symbol) -> Result<Value> {
        self.dirs
            .write()
            .unwrap()
            .remove(&name)
            .map_or_else(|| cx.factory().nil(), |dir| cx.factory().opaque(dir))
    }
    fn is_dir(&self, _cx: &mut Cx, name: Symbol) -> Result<bool> {
        Ok(self.dirs.read().unwrap().contains_key(&name))
    }
}

fn context() -> Cx {
    let (mut cx, seat) = Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    seat.grant(&mut cx, module_load_capability()).unwrap();
    seat.grant(&mut cx, read_eval_capability()).unwrap();
    cx.load_lib(&LispCodecLib::new(sim_kernel::CodecId(31)).unwrap())
        .unwrap();
    cx
}

fn request(
    root: Arc<MemoryDir>,
    specifier: &str,
    importer: Option<ModuleIdentity>,
) -> ModuleRequest {
    ModuleRequest {
        root_id: Symbol::new("fixture"),
        root,
        importer,
        specifier: specifier.to_owned(),
        codec: Symbol::qualified("codec", "lisp"),
        read_policy: ReadPolicy {
            trust: TrustLevel::TrustedSource,
            capabilities: CapabilitySet::new().grant(read_eval_capability()),
        },
        requires: vec![module_load_capability()],
        allow: CapabilitySet::new()
            .grant(read_eval_capability())
            .grant(module_load_capability()),
    }
}

fn value_expr(cx: &mut Cx, module: &ModuleInstance) -> Expr {
    module
        .default_export()
        .get()
        .unwrap()
        .object()
        .as_expr(cx)
        .unwrap()
}

#[test]
fn relative_resolution_and_root_escape_are_explicit() {
    let mut cx = context();
    let root = Arc::new(MemoryDir::default());
    let pkg = root.directory(&mut cx, "pkg");
    pkg.source(&mut cx, "sibling.sim", "\"relative\"");
    let importer = ModuleIdentity {
        root: Symbol::new("fixture"),
        path: "pkg/main.sim".to_owned(),
    };
    let loader = ModuleLoader::new();
    let loaded = loader
        .load(
            &mut cx,
            request(root.clone(), "./sibling.sim", Some(importer.clone())),
        )
        .unwrap();
    assert_eq!(loaded.identity.path(), "pkg/sibling.sim");
    assert_eq!(
        value_expr(&mut cx, &loaded),
        Expr::String("relative".to_owned())
    );
    let error = loader
        .load(&mut cx, request(root, "../../escape.sim", Some(importer)))
        .unwrap_err();
    assert!(error.to_string().contains("escapes supplied root"));
}

#[test]
fn failure_is_cached_and_receipted_deterministically() {
    let mut cx = context();
    let root = Arc::new(MemoryDir::default());
    root.source(&mut cx, "bad.sim", "(");
    let loader = ModuleLoader::new();
    let first = loader
        .load(&mut cx, request(root.clone(), "bad.sim", None))
        .unwrap_err()
        .to_string();
    root.source(&mut cx, "bad.sim", "\"repaired but cached\"");
    let second = loader
        .load(&mut cx, request(root, "bad.sim", None))
        .unwrap_err()
        .to_string();
    assert_eq!(first, second);
    assert_eq!(
        loader
            .receipts()
            .unwrap()
            .iter()
            .map(|r| r.outcome)
            .collect::<Vec<_>>(),
        vec![
            ModuleResolutionOutcome::Failed,
            ModuleResolutionOutcome::Failed
        ]
    );
}

#[test]
fn replacement_updates_existing_live_binding() {
    let mut cx = context();
    let root = Arc::new(MemoryDir::default());
    root.source(&mut cx, "live.sim", "\"one\"");
    let loader = ModuleLoader::new();
    let first = loader
        .load(&mut cx, request(root.clone(), "live.sim", None))
        .unwrap();
    let edge = first.default_export().clone();
    root.source(&mut cx, "live.sim", "\"two\"");
    let second = loader
        .reload(&mut cx, request(root, "live.sim", None))
        .unwrap();
    assert_eq!(second.generation(), 2);
    assert_eq!(
        edge.get().unwrap().object().as_expr(&mut cx).unwrap(),
        Expr::String("two".to_owned())
    );
}

#[test]
fn initializing_cycle_has_stable_receipt_without_storage_access() {
    let mut cx = context();
    let root = Arc::new(MemoryDir::default());
    let loader = ModuleLoader::new();
    let identity = ModuleIdentity {
        root: Symbol::new("fixture"),
        path: "cycle.sim".to_owned(),
    };
    loader.state.lock().unwrap().cache.insert(
        identity.clone(),
        CacheState::Initializing {
            owner: std::thread::current().id(),
            generation: 7,
        },
    );
    let error = loader
        .load(&mut cx, request(root, "cycle.sim", None))
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "evaluation error: module cycle at fixture:cycle.sim"
    );
    let receipt = loader.receipts().unwrap().pop().unwrap();
    assert_eq!(
        (receipt.generation, receipt.outcome),
        (7, ModuleResolutionOutcome::Cycle)
    );
}

#[test]
fn cache_hit_reuses_one_linked_generation() {
    let mut cx = context();
    let root = Arc::new(MemoryDir::default());
    root.source(&mut cx, "shared.sim", "\"shared\"");
    let loader = ModuleLoader::new();
    let first = loader
        .load(&mut cx, request(root.clone(), "shared.sim", None))
        .unwrap();
    let second = loader
        .load(&mut cx, request(root, "shared.sim", None))
        .unwrap();
    assert_eq!(first.generation(), second.generation());
    assert_eq!(
        loader.receipts().unwrap().last().unwrap().outcome,
        ModuleResolutionOutcome::CacheHit
    );
}

#[test]
fn concurrent_requests_share_one_initialization() {
    let mut seed_cx = context();
    let root = Arc::new(MemoryDir::default());
    root.source(&mut seed_cx, "concurrent.sim", "\"once\"");
    let loader = Arc::new(ModuleLoader::new());
    let workers = (0..8)
        .map(|_| {
            let root = root.clone();
            let loader = loader.clone();
            std::thread::spawn(move || {
                let mut cx = context();
                loader
                    .load(&mut cx, request(root, "concurrent.sim", None))
                    .unwrap()
                    .generation()
            })
        })
        .collect::<Vec<_>>();
    assert!(
        workers
            .into_iter()
            .all(|worker| worker.join().unwrap() == 1)
    );
    let receipts = loader.receipts().unwrap();
    assert_eq!(
        receipts
            .iter()
            .filter(|r| r.outcome == ModuleResolutionOutcome::Linked)
            .count(),
        1
    );
    assert_eq!(
        receipts
            .iter()
            .filter(|r| r.outcome == ModuleResolutionOutcome::CacheHit)
            .count(),
        7
    );
}

#[derive(Debug)]
struct CompositionNode;

impl ManagedObject for CompositionNode {
    fn trace_edges(&self, _visitor: &mut dyn EdgeVisitor) {}

    fn clear_weak_edge(&mut self, _edge: EdgeId, _expected: ManagedId) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CompositionJob {
    Microtask,
    Finalization,
}

#[test]
fn language_neutral_organs_compose_through_one_source_module_lifecycle() {
    let mut cx = context();
    let root = Arc::new(MemoryDir::default());
    root.source(&mut cx, "component.sim", "\"generation-one\"");

    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(2).unwrap());
    let managed = arena.allocate(CompositionNode).unwrap();
    let rooted = arena.root(managed).unwrap();

    let parameter = Symbol::new("component");
    let argument = cx.factory().string("bound-component".to_owned()).unwrap();
    let bound = CallSignature::new()
        .with_positional(vec![CallParameter::required(parameter.clone())])
        .bind([CallArgument::Positional(argument.clone())])
        .unwrap();
    assert!(
        bound
            .get(&parameter)
            .is_some_and(|value| value == &argument)
    );

    let mut properties: PropertyStore<&str, &str, u64, ()> = PropertyStore::default();
    let generation = "generation";
    properties
        .define(
            &"component",
            generation,
            Descriptor::Data(DataDescriptor {
                value: 1,
                writable: true,
                enumerable: true,
                configurable: false,
            }),
        )
        .unwrap();
    assert!(matches!(
        properties.own(&"component", &generation),
        Some(Descriptor::Data(DataDescriptor { value, configurable: false, .. }))
            if *value == managed.id().allocation_ordinal() + 1
    ));

    let loader = ModuleLoader::new();
    let first = loader
        .load(&mut cx, request(root.clone(), "component.sim", None))
        .unwrap();
    let live_export = first.default_export().clone();
    assert_eq!(
        value_expr(&mut cx, &first),
        Expr::String("generation-one".to_owned())
    );

    let mut frame = ResumableFrame::new(
        FrameLimits { depth: 1, work: 2 },
        |packet: ResumePacket<u64, ()>, budget: &mut sim_lib_control::StepBudget| {
            budget.charge_work()?;
            match packet {
                ResumePacket::Start => Ok(ResumeResult::Yielded(1)),
                ResumePacket::Send(generation) => Ok(ResumeResult::Returned(generation)),
                ResumePacket::Throw(()) => Ok(ResumeResult::Failed(())),
                ResumePacket::Close => Ok(ResumeResult::Returned(0)),
            }
        },
    );
    assert_eq!(
        frame.resume(ResumePacket::Start),
        Ok(ResumeResult::Yielded(1))
    );

    let mut jobs = JobQueues::new(AdmissionLimit(3));
    jobs.enqueue(CompositionJob::Microtask, |jobs| {
        jobs.enqueue(CompositionJob::Microtask, |_| {}).unwrap();
    })
    .unwrap();
    jobs.enqueue(CompositionJob::Finalization, |_| {}).unwrap();
    let checkpoint = jobs
        .checkpoint(CompositionJob::Microtask, WorkLimit(2))
        .unwrap();
    assert_eq!(checkpoint.completed.len(), 2);
    assert!(
        jobs.drain(CompositionJob::Finalization, WorkLimit(0))
            .completed
            .is_empty()
    );

    root.source(&mut cx, "component.sim", "\"generation-two\"");
    let second = loader
        .reload(&mut cx, request(root, "component.sim", None))
        .unwrap();
    assert_eq!(
        frame.resume(ResumePacket::Send(second.generation())),
        Ok(ResumeResult::Returned(2))
    );
    assert_eq!(
        live_export
            .get()
            .unwrap()
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::String("generation-two".to_owned())
    );

    let (_, safepoint) = arena
        .safepoint(|snapshot| snapshot.objects().count())
        .unwrap();
    assert_eq!(safepoint.roots, vec![managed.id()]);
    arena.release_root(rooted).unwrap();
    let teardown = arena.teardown();
    assert_eq!(teardown.objects, vec![managed.id()]);
    assert_eq!(
        loader
            .receipts()
            .unwrap()
            .iter()
            .map(|receipt| receipt.outcome)
            .collect::<Vec<_>>(),
        [
            ModuleResolutionOutcome::Linked,
            ModuleResolutionOutcome::Linked
        ]
    );
}
