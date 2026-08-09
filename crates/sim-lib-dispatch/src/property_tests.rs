// conformance: ordered language-neutral data and accessor descriptor mechanics.

use std::convert::Infallible;

use crate::*;

type Store = PropertyStore<&'static str, &'static str, i32, &'static str>;

fn data(value: i32, writable: bool, configurable: bool) -> Descriptor<i32, &'static str> {
    Descriptor::Data(DataDescriptor {
        value,
        writable,
        enumerable: true,
        configurable,
    })
}

fn accessor(get: Option<&'static str>, set: Option<&'static str>) -> Descriptor<i32, &'static str> {
    Descriptor::Accessor(AccessorDescriptor {
        get,
        set,
        enumerable: true,
        configurable: true,
    })
}

#[derive(Default)]
struct Hooks {
    calls: Vec<(AccessKind, &'static str, &'static str)>,
    set_value: Option<i32>,
}

impl PropertyHook<&'static str, &'static str, i32, &'static str> for Hooks {
    type Error = Infallible;

    fn get(
        &mut self,
        _context: &mut AccessContext<&'static str, &'static str>,
        hook: &&'static str,
        receiver: &&'static str,
        key: &&'static str,
    ) -> Result<i32, AccessError<Self::Error>> {
        self.calls.push((AccessKind::Get, receiver, key));
        Ok(if *hook == "forty-two" { 42 } else { 0 })
    }

    fn set(
        &mut self,
        _context: &mut AccessContext<&'static str, &'static str>,
        _hook: &&'static str,
        receiver: &&'static str,
        key: &&'static str,
        value: i32,
    ) -> Result<(), AccessError<Self::Error>> {
        self.calls.push((AccessKind::Set, receiver, key));
        self.set_value = Some(value);
        Ok(())
    }
}

#[test]
fn own_records_define_delete_and_keep_deterministic_order() {
    let mut store = Store::default();
    store.define(&"object", "b", data(2, true, true)).unwrap();
    store.define(&"object", "a", data(1, true, true)).unwrap();
    store.define(&"object", "b", data(3, true, true)).unwrap();
    assert_eq!(store.own_keys(&"object", false), ["b", "a"]);
    assert!(store.delete(&"object", &"b").unwrap());
    store.define(&"object", "b", data(4, true, true)).unwrap();
    assert_eq!(store.own_keys(&"object", false), ["a", "b"]);

    store
        .define(&"object", "fixed", data(7, false, false))
        .unwrap();
    assert_eq!(
        store.define(&"object", "fixed", data(8, false, false)),
        Err(DefineError::InvariantViolation)
    );
    assert_eq!(
        store.delete(&"object", &"fixed"),
        Err(DefineError::InvariantViolation)
    );
}

#[test]
fn inherited_accessors_receive_original_receiver() {
    let mut store = Store::default();
    store
        .define(
            &"type",
            "answer",
            accessor(Some("forty-two"), Some("capture")),
        )
        .unwrap();
    let mut hooks = Hooks::default();
    let mut context = AccessContext::new(8);
    assert_eq!(
        store
            .get(
                &["instance", "type"],
                &"instance",
                &"answer",
                &mut context,
                &mut hooks,
            )
            .unwrap(),
        Some(42)
    );
    assert!(
        store
            .set(
                &["instance", "type"],
                &"instance",
                &"answer",
                9,
                &mut context,
                &mut hooks,
            )
            .unwrap()
    );
    assert_eq!(
        hooks.calls,
        [
            (AccessKind::Get, "instance", "answer"),
            (AccessKind::Set, "instance", "answer"),
        ]
    );
    assert_eq!(hooks.set_value, Some(9));
}

#[test]
fn explicit_owner_order_is_bounded_and_cycle_safe() {
    let mut store = Store::default();
    store.define(&"root", "key", data(5, true, true)).unwrap();
    let mut hooks = Hooks::default();
    let mut context = AccessContext::new(4);
    assert_eq!(
        store
            .get(
                &["child", "child", "root"],
                &"child",
                &"key",
                &mut context,
                &mut hooks,
            )
            .unwrap(),
        Some(5)
    );
    assert_eq!(context.remaining(), 1);

    let mut exhausted = AccessContext::new(1);
    assert_eq!(
        store.get(
            &["child", "root"],
            &"child",
            &"key",
            &mut exhausted,
            &mut hooks,
        ),
        Err(AccessError::BudgetExhausted)
    );
}

#[test]
fn interception_reentry_is_rejected_within_shared_budget() {
    let mut context = AccessContext::new(3);
    let result: Result<(), AccessError<Infallible>> =
        context.intercept(AccessKind::Get, &"receiver", &"key", |context| {
            context.intercept(AccessKind::Get, &"receiver", &"key", |_| Ok(()))
        });
    assert_eq!(result, Err(AccessError::RecursiveReentry));
    assert_eq!(context.remaining(), 1);
}

#[test]
fn caller_policy_can_model_type_bound_data_and_non_data_precedence() {
    let mut store = Store::default();
    store
        .define(&"type", "data", accessor(Some("forty-two"), Some("set")))
        .unwrap();
    store
        .define(&"instance", "data", data(1, true, true))
        .unwrap();
    store
        .define(&"type", "non-data", accessor(Some("forty-two"), None))
        .unwrap();
    store
        .define(&"instance", "non-data", data(2, true, true))
        .unwrap();
    let mut hooks = Hooks::default();

    // A profile chooses type-first for a data descriptor.
    assert_eq!(
        store
            .get(
                &["type", "instance"],
                &"instance",
                &"data",
                &mut AccessContext::new(8),
                &mut hooks,
            )
            .unwrap(),
        Some(42)
    );
    // The same profile chooses instance-first for a non-data descriptor.
    assert_eq!(
        store
            .get(
                &["instance", "type"],
                &"instance",
                &"non-data",
                &mut AccessContext::new(8),
                &mut hooks,
            )
            .unwrap(),
        Some(2)
    );
}

#[test]
fn caller_policy_can_model_prototype_lookup_and_own_shadowing() {
    let mut store = Store::default();
    store
        .define(&"prototype", "name", data(10, true, true))
        .unwrap();
    let mut hooks = Hooks::default();
    let owners = ["object", "prototype"];
    assert_eq!(
        store
            .get(
                &owners,
                &"object",
                &"name",
                &mut AccessContext::new(4),
                &mut hooks,
            )
            .unwrap(),
        Some(10)
    );
    store
        .define(&"object", "name", data(20, true, true))
        .unwrap();
    assert_eq!(
        store
            .get(
                &owners,
                &"object",
                &"name",
                &mut AccessContext::new(4),
                &mut hooks,
            )
            .unwrap(),
        Some(20)
    );
}
