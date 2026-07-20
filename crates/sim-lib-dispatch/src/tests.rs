use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use sim_kernel::{
    CapabilityName, Cx, Expr, HintMetadata, Ref, Result, Symbol, Value,
    card::{card_for_ref, card_kind_predicate},
    force_list_to_vec,
    standard::standard_organ_kind,
};
use sim_shape::{AnyShape, ExprKindShape};

use crate::*;

use sim_kernel::testing::bare_cx as cx;

fn string(cx: &mut Cx, value: &str) -> Value {
    cx.factory().string(value.to_owned()).unwrap()
}

fn bool_value(cx: &mut Cx, value: bool) -> Value {
    cx.factory().bool(value).unwrap()
}

fn body(label: &'static str) -> MethodBody {
    Arc::new(move |cx, _args| cx.factory().string(label.to_owned()))
}

fn trace_body(label: &'static str, trace: Arc<Mutex<Vec<String>>>) -> MethodBody {
    Arc::new(move |cx, _args| {
        trace.lock().unwrap().push(label.to_owned());
        cx.factory().string(label.to_owned())
    })
}

fn string_shape() -> Arc<dyn sim_kernel::Shape> {
    Arc::new(ExprKindShape::new(sim_kernel::ExprKind::String))
}

fn any_shape() -> Arc<dyn sim_kernel::Shape> {
    Arc::new(AnyShape)
}

fn primary(id: &'static str, shapes: Vec<Arc<dyn sim_kernel::Shape>>) -> DispatchMethod {
    DispatchMethod::new(
        Symbol::qualified("method", id),
        MethodRole::Primary,
        shapes,
        body(id),
    )
}

#[test]
fn operation_hints_include_method_metadata() {
    let mut generic = GenericFunction::new(Symbol::qualified("dispatch-test", "hints"));
    generic
        .add_method(
            primary("string", vec![string_shape()])
                .with_argument_hint(Symbol::new("input"), "value to classify")
                .with_capability_requirement(CapabilityName::new("dispatch.inspect"))
                .with_codec_safe_form(Symbol::qualified("codec", "lisp"))
                .with_example("(dispatch-test/hints input)"),
        )
        .unwrap();

    let text = generic
        .operation_hints()
        .iter()
        .map(HintMetadata::radar_text)
        .collect::<Vec<_>>()
        .join(" ");

    assert!(text.contains("runtime-hint/argument"));
    assert!(text.contains("dispatch.inspect"));
    assert!(text.contains("codec/lisp"));
    assert!(text.contains("(dispatch-test/hints input)"));
}

#[test]
fn failed_selection_pushes_radar_consumable_hints() {
    let mut cx = cx();
    let mut generic = GenericFunction::new(Symbol::qualified("dispatch-test", "diagnose"));
    generic
        .add_method(
            primary("string", vec![string_shape()])
                .with_argument_hint(Symbol::new("input"), "string value"),
        )
        .unwrap();

    let arg = bool_value(&mut cx, true);
    assert!(generic.call(&mut cx, &[arg]).is_err());

    let diagnostics = cx.diagnostics().messages();
    let hints = HintMetadata::collect_from_diagnostic(&diagnostics[0]);
    let text = hints
        .iter()
        .map(HintMetadata::radar_text)
        .collect::<Vec<_>>()
        .join(" ");

    assert!(text.contains("runtime-hint/overload-selection"));
    assert!(text.contains("runtime-hint/argument"));
    assert!(text.contains("string value"));
}

#[test]
fn most_specific_multimethod_is_selected() {
    let mut cx = cx();
    let mut generic = GenericFunction::new(Symbol::qualified("dispatch-test", "choose"));
    generic
        .add_method(primary("broad", vec![any_shape(), any_shape()]))
        .unwrap();
    generic
        .add_method(primary("second-string", vec![any_shape(), string_shape()]))
        .unwrap();

    let args = [bool_value(&mut cx, true), string(&mut cx, "text")];
    let selected = generic.select_primary(&mut cx, &args).unwrap();
    assert_eq!(
        selected.method(),
        &Symbol::qualified("method", "second-string")
    );

    let result = generic.call(&mut cx, &args).unwrap();
    assert_eq!(
        result.object().as_expr(&mut cx).unwrap(),
        Expr::String("second-string".to_owned())
    );
}

#[test]
fn method_combination_order_is_around_before_primary_after() -> Result<()> {
    let mut cx = cx();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let mut generic = GenericFunction::new(Symbol::qualified("dispatch-test", "combine"));
    for (id, role, shape) in [
        ("around-any", MethodRole::Around, any_shape()),
        ("around-string", MethodRole::Around, string_shape()),
        ("before-any", MethodRole::Before, any_shape()),
        ("before-string", MethodRole::Before, string_shape()),
        ("primary-string", MethodRole::Primary, string_shape()),
        ("after-any", MethodRole::After, any_shape()),
        ("after-string", MethodRole::After, string_shape()),
    ] {
        generic.add_method(DispatchMethod::new(
            Symbol::qualified("method", id),
            role,
            vec![shape],
            trace_body(id, trace.clone()),
        ))?;
    }

    let args = [string(&mut cx, "text")];
    let order = generic.dispatch_order(&mut cx, &args)?;
    assert_eq!(
        order,
        vec![
            Symbol::qualified("method", "around-string"),
            Symbol::qualified("method", "around-any"),
            Symbol::qualified("method", "before-string"),
            Symbol::qualified("method", "before-any"),
            Symbol::qualified("method", "primary-string"),
            Symbol::qualified("method", "after-any"),
            Symbol::qualified("method", "after-string"),
        ]
    );

    let result = generic.call(&mut cx, &args)?;
    assert_eq!(
        result.object().as_expr(&mut cx).unwrap(),
        Expr::String("primary-string".to_owned())
    );
    assert_eq!(
        *trace.lock().unwrap(),
        vec![
            "around-string",
            "around-any",
            "before-string",
            "before-any",
            "primary-string",
            "after-any",
            "after-string",
        ]
    );
    Ok(())
}

#[test]
fn specificity_is_inspectable() {
    let mut cx = cx();
    let mut generic = GenericFunction::new(Symbol::qualified("dispatch-test", "inspect"));
    generic
        .add_method(primary("any", vec![any_shape()]))
        .unwrap();
    generic
        .add_method(primary("string", vec![string_shape()]))
        .unwrap();

    let args = [string(&mut cx, "text")];
    let inspected = generic.inspect_specificity(&mut cx, &args).unwrap();
    assert_eq!(inspected.len(), 2);
    assert_eq!(
        inspected[0].method(),
        &Symbol::qualified("method", "string")
    );
    assert_eq!(inspected[1].method(), &Symbol::qualified("method", "any"));
    assert!(inspected[0].score() > inspected[1].score());
    assert_eq!(
        inspected[0].argument_scores(),
        &[sim_kernel::MatchScore::exact(10)]
    );
}

#[test]
fn cl_julia_and_clojure_profiles_reuse_one_generic() -> Result<()> {
    let mut cx = cx();
    let mut generic = GenericFunction::new(Symbol::qualified("dispatch-test", "shared"));
    generic.add_method(primary("shared-string", vec![string_shape()]))?;
    let profiles = [
        Symbol::qualified("profile", "common-lisp-lite"),
        Symbol::qualified("profile", "julia-lite"),
        Symbol::qualified("profile", "clojure-core"),
    ];

    for profile in profiles {
        let args = [string(&mut cx, "text")];
        let result = generic.call_for_profile(&mut cx, &profile, &args)?;
        assert_eq!(
            result.object().as_expr(&mut cx).unwrap(),
            Expr::String("shared-string".to_owned())
        );
    }
    Ok(())
}

#[test]
fn meta_index_reads_table_backed_meta_slot_without_baked_in_slot_names() -> Result<()> {
    let mut cx = cx();
    let mut protocol = TestMetaProtocol::default();
    let receiver = string(&mut cx, "receiver");
    let meta_table = string(&mut cx, "meta-table");
    let direct_key = string(&mut cx, "direct");
    let meta_key = string(&mut cx, "fallback");
    let slot = Symbol::qualified("dispatch-test", "index-slot");
    let direct_value = string(&mut cx, "raw");
    let shadow_value = string(&mut cx, "shadow");
    let fallback_value = string(&mut cx, "meta");

    protocol.insert_raw(&receiver, &direct_key, direct_value.clone());
    protocol.insert_meta(&receiver, &slot, meta_table.clone());
    protocol.insert_raw(&meta_table, &direct_key, shadow_value);
    protocol.insert_raw(&meta_table, &meta_key, fallback_value.clone());

    let raw = meta_index(&mut cx, &protocol, &receiver, &direct_key, &slot)?.unwrap();
    assert_eq!(
        raw.object().as_expr(&mut cx).unwrap(),
        direct_value.object().as_expr(&mut cx).unwrap()
    );

    let fallback = meta_index(&mut cx, &protocol, &receiver, &meta_key, &slot)?.unwrap();
    assert_eq!(
        fallback.object().as_expr(&mut cx).unwrap(),
        fallback_value.object().as_expr(&mut cx).unwrap()
    );
    Ok(())
}

#[test]
fn meta_index_supports_function_backed_meta_slot() -> Result<()> {
    let mut cx = cx();
    let mut protocol = TestMetaProtocol::default();
    let receiver = string(&mut cx, "receiver");
    let key = string(&mut cx, "computed-key");
    let slot = Symbol::qualified("dispatch-test", "call-slot");
    let function = string(&mut cx, "function-meta");
    let computed = string(&mut cx, "computed");
    let reply = computed.clone();

    protocol.insert_meta(&receiver, &slot, function.clone());
    protocol.insert_function(
        function,
        Arc::new(move |_cx, _receiver, _key| Ok(Some(reply.clone()))),
    );

    let result = meta_index(&mut cx, &protocol, &receiver, &key, &slot)?.unwrap();
    assert_eq!(
        result.object().as_expr(&mut cx).unwrap(),
        computed.object().as_expr(&mut cx).unwrap()
    );
    Ok(())
}

#[test]
fn meta_index_walks_prototype_chain_through_protocol_override() -> Result<()> {
    let mut cx = cx();
    let mut protocol = TestMetaProtocol::recursive();
    let child = string(&mut cx, "child");
    let parent = string(&mut cx, "parent");
    let grandparent = string(&mut cx, "grandparent");
    let key = string(&mut cx, "inherited-key");
    let slot = Symbol::qualified("dispatch-test", "parent-slot");
    let inherited = string(&mut cx, "inherited");

    protocol.insert_meta(&child, &slot, parent.clone());
    protocol.insert_meta(&parent, &slot, grandparent.clone());
    protocol.insert_raw(&grandparent, &key, inherited.clone());

    let result = meta_index(&mut cx, &protocol, &child, &key, &slot)?.unwrap();
    assert_eq!(
        result.object().as_expr(&mut cx).unwrap(),
        inherited.object().as_expr(&mut cx).unwrap()
    );
    Ok(())
}

#[test]
fn dispatch_organ_claims_project_to_card() {
    let mut cx = cx();
    publish_dispatch_organ_claims(&mut cx).unwrap();

    let claims = cx
        .query_facts(sim_kernel::ClaimPattern {
            subject: Some(Ref::Symbol(dispatch_organ_symbol())),
            predicate: Some(card_kind_predicate()),
            object: Some(Ref::Symbol(standard_organ_kind())),
            include_revoked: false,
        })
        .unwrap();
    assert_eq!(claims.len(), 1);

    let card = card_for_ref(&mut cx, Ref::Symbol(dispatch_organ_symbol())).unwrap();
    let table = card.object().as_table(&mut cx).unwrap();
    let entries = table.object().as_table_impl().unwrap();
    let ops = entries.get(&mut cx, Symbol::new("ops")).unwrap();
    let list = ops.object().as_list().unwrap();
    let values = force_list_to_vec(&mut cx, list, "dispatch organ ops").unwrap();

    assert!(values.into_iter().any(|value| {
        value.object().as_expr(&mut cx).unwrap()
            == Expr::Symbol(Symbol::qualified("dispatch", "specificity.v1"))
    }));
}

type MetaFunction = Arc<dyn Fn(&mut Cx, &Value, &Value) -> Result<Option<Value>> + Send + Sync>;

#[derive(Default)]
struct TestMetaProtocol {
    raw: HashMap<Value, HashMap<Value, Value>>,
    meta: HashMap<(Value, Symbol), Value>,
    functions: HashMap<Value, MetaFunction>,
    recursive: bool,
}

impl TestMetaProtocol {
    fn recursive() -> Self {
        Self {
            recursive: true,
            ..Self::default()
        }
    }

    fn insert_raw(&mut self, value: &Value, key: &Value, result: Value) {
        self.raw
            .entry(value.clone())
            .or_default()
            .insert(key.clone(), result);
    }

    fn insert_meta(&mut self, value: &Value, slot: &Symbol, result: Value) {
        self.meta.insert((value.clone(), slot.clone()), result);
    }

    fn insert_function(&mut self, value: Value, function: MetaFunction) {
        self.functions.insert(value, function);
    }
}

impl MetaObjectProtocol for TestMetaProtocol {
    fn raw_get(&self, _cx: &mut Cx, value: &Value, key: &Value) -> Result<Option<Value>> {
        Ok(self
            .raw
            .get(value)
            .and_then(|table| table.get(key).cloned()))
    }

    fn get_meta(&self, _cx: &mut Cx, value: &Value, slot: &Symbol) -> Result<Option<Value>> {
        Ok(self.meta.get(&(value.clone(), slot.clone())).cloned())
    }

    fn apply_meta(
        &self,
        cx: &mut Cx,
        receiver: &Value,
        key: &Value,
        index_slot: &Symbol,
        meta_value: &Value,
    ) -> Result<Option<Value>> {
        if let Some(function) = self.functions.get(meta_value) {
            return function(cx, receiver, key);
        }
        if self.recursive {
            return meta_index(cx, self, meta_value, key, index_slot);
        }
        self.raw_get(cx, meta_value, key)
    }
}

// ---- generic function as a runtime callable value ----

#[test]
fn generic_value_dispatches_most_specific_when_called() {
    let mut cx = cx();
    let mut generic = GenericFunction::new(Symbol::qualified("dispatch-test", "runtime-choose"));
    generic
        .add_method(primary("broad", vec![any_shape(), any_shape()]))
        .unwrap();
    generic
        .add_method(primary("second-string", vec![any_shape(), string_shape()]))
        .unwrap();

    // Wrapped as a runtime value, it is an ordinary callable.
    let value = generic_function_value(&mut cx, generic).unwrap();
    assert!(value.object().as_callable().is_some());

    // Calling through the general call path dispatches most-specific: the second
    // argument is a string, so the (any, string) method wins over (any, any).
    let arg0 = bool_value(&mut cx, true);
    let arg1 = string(&mut cx, "text");
    let result = cx
        .call_value(value.clone(), sim_kernel::Args::new(vec![arg0, arg1]))
        .unwrap();
    let Expr::String(label) = result.object().as_expr(&mut cx).unwrap() else {
        panic!("expected the method body's label string");
    };
    assert_eq!(label, "second-string");

    // With two non-string arguments the broad (any, any) method is selected.
    let a = bool_value(&mut cx, true);
    let b = bool_value(&mut cx, false);
    let broad = cx
        .call_value(value, sim_kernel::Args::new(vec![a, b]))
        .unwrap();
    let Expr::String(label) = broad.object().as_expr(&mut cx).unwrap() else {
        panic!("expected label string");
    };
    assert_eq!(label, "broad");
}
