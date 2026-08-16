#[cfg(test)]
mod tests {
    use super::*;
    use sim_lib_binding::BindingCell;
    use sim_lib_function::{CallMode, CaptureDescriptor, ParameterDescriptor};
    use sim_lib_gc_tracing::CollectionLimits;
    fn model() -> JavascriptObjects {
        JavascriptObjects::new(
            JavascriptHeap::tracing(
                32,
                CollectionLimits {
                    objects: 32,
                    edges: 64,
                    stack: 32,
                    work: 256,
                    clears: 32,
                    finalizers: 0,
                },
            )
            .unwrap(),
        )
    }
    fn s(v: &str) -> JavascriptPropertyKey {
        JavascriptPropertyKey::String(v.into())
    }
    fn function(
        kind: JavascriptFunctionKind,
        environment: ManagedHandle,
        constructable: bool,
        private_names: Vec<String>,
    ) -> JavascriptFunction {
        let capture_name = Symbol::new("environment");
        JavascriptFunction::new(
            FunctionPlan::new(
                Symbol::new("javascript:fixture"),
                vec![],
                vec![CaptureDescriptor::new(capture_name.clone(), None)],
                None,
            )
            .unwrap(),
            vec![CapturedBinding::new(
                BindingCell::uninitialized(capture_name),
                environment,
            )],
            JavascriptFunctionPolicy {
                kind,
                constructable,
                defaults: BTreeMap::new(),
                asynchronous: false,
                generator: false,
                realm: Symbol::new("realm:fixture"),
                error_origin: "fixture.js:1".into(),
            },
            private_names,
        )
        .unwrap()
    }
    #[test]
    fn descriptors_prototypes_arrays_and_private_names_are_shared_mechanics() {
        let mut m = model();
        let env = m.ordinary().unwrap();
        let class = m
            .function(
                function(
                    JavascriptFunctionKind::ClassConstructor,
                    env,
                    true,
                    vec!["x".into()],
                ),
                None,
            )
            .unwrap();
        m.define_data(
            class,
            s("inherited"),
            JavascriptValue::Number(7.0),
            false,
            true,
            false,
        )
        .unwrap();
        let o = m.construct(class).unwrap();
        assert_eq!(
            m.get(o, &s("inherited"), 8).unwrap(),
            Some(JavascriptValue::Number(7.0))
        );
        m.define_data(o, s("10"), JavascriptValue::Null, true, true, true)
            .unwrap();
        m.define_data(o, s("2"), JavascriptValue::Null, true, true, true)
            .unwrap();
        m.define_accessor(
            o,
            s("answer"),
            Some(JavascriptValue::Number(42.0)),
            false,
            true,
            true,
        )
        .unwrap();
        assert_eq!(m.enumerable_keys(o), vec![s("2"), s("10"), s("answer")]);
        assert_eq!(
            m.get(o, &s("answer"), 8).unwrap(),
            Some(JavascriptValue::Number(42.0))
        );
        assert!(m.private_key(class, "x").is_ok());
        assert!(m.private_key(class, "y").is_err());
        assert!(m.delete(o, &s("answer")).unwrap());
    }

    #[test]
    fn error_cause_and_aggregate_members_remain_ordered_object_properties() {
        let mut m = model();
        let error = m.ordinary().unwrap();
        let aggregate = m.ordinary().unwrap();
        let members = m.ordinary().unwrap();
        let cause = JavascriptValue::String("root".into());
        m.define_data(error, s("cause"), cause.clone(), true, false, true)
            .unwrap();
        m.define_data(
            members,
            s("0"),
            JavascriptValue::String("first".into()),
            true,
            true,
            true,
        )
        .unwrap();
        m.define_data(
            members,
            s("1"),
            JavascriptValue::String("second".into()),
            true,
            true,
            true,
        )
        .unwrap();
        m.define_data(
            aggregate,
            s("errors"),
            JavascriptValue::Managed(members),
            true,
            false,
            true,
        )
        .unwrap();

        assert_eq!(m.get(error, &s("cause"), 4).unwrap(), Some(cause));
        assert!(m.enumerable_keys(error).is_empty());
        assert!(m.enumerable_keys(aggregate).is_empty());
        assert_eq!(m.enumerable_keys(members), vec![s("0"), s("1")]);
        assert_eq!(
            m.get(members, &s("0"), 4).unwrap(),
            Some(JavascriptValue::String("first".into()))
        );
        assert_eq!(
            m.get(members, &s("1"), 4).unwrap(),
            Some(JavascriptValue::String("second".into()))
        );
    }
    #[test]
    fn functions_arrows_construction_shapes_and_gaps_are_explicit() {
        let mut m = model();
        let env = m.ordinary().unwrap();
        let arrow = m
            .function(
                function(JavascriptFunctionKind::Arrow, env, false, vec![]),
                Some(JavascriptValue::String("lexical".into())),
            )
            .unwrap();
        assert_eq!(
            m.call_this(arrow, JavascriptValue::String("dynamic".into()))
                .unwrap(),
            JavascriptThis::Lexical(JavascriptValue::String("lexical".into()))
        );
        let called = m
            .call(
                arrow,
                JavascriptValue::Undefined,
                &[JavascriptValue::Number(42.0)],
                |plan, captures, this, arguments| {
                    Ok::<_, JavascriptObjectError>((
                        plan.clone(),
                        captures[0].managed(),
                        this,
                        arguments[0].clone(),
                    ))
                },
            )
            .unwrap();
        assert_eq!(called.1, env);
        assert_eq!(called.3, JavascriptValue::Number(42.0));
        assert_eq!(
            m.construct(arrow),
            Err(JavascriptObjectError::NotConstructor)
        );
        assert!(javascript_callable_shape_constraints().is_empty());
        assert_eq!(javascript_object_gaps().len(), 2);
    }
    #[test]
    fn mixed_language_cycles_reclaim_without_changing_observed_values() {
        let mut m = model();
        let env = m.ordinary().unwrap();
        let f = m
            .function(
                function(JavascriptFunctionKind::Function, env, true, vec![]),
                None,
            )
            .unwrap();
        let array = m.ordinary().unwrap();
        m.set_prototype(array, f).unwrap();
        m.define_accessor(
            array,
            s("stable"),
            Some(JavascriptValue::Number(42.0)),
            false,
            true,
            true,
        )
        .unwrap();
        assert_eq!(
            m.get(array, &s("stable"), 8).unwrap(),
            Some(JavascriptValue::Number(42.0))
        );
        assert_eq!(m.live_len(), 3);
        assert_eq!(m.collect().unwrap().unwrap().swept.len(), 3);
        assert_eq!(m.live_len(), 0);
    }

    #[test]
    fn shared_plan_is_form_neutral_while_javascript_policy_preserves_semantics() {
        let mut m = model();
        let environment = m.ordinary().unwrap();
        let parameter = ParameterDescriptor::new(
            Symbol::new("head"),
            ParameterKind::Optional,
            CallMode::POSITIONAL,
            None,
        );
        let rest = ParameterDescriptor::new(
            Symbol::new("tail"),
            ParameterKind::Remainder,
            CallMode::POSITIONAL,
            None,
        );
        let capture_name = Symbol::new("closed");
        let plan = FunctionPlan::new(
            Symbol::new("javascript:same-declaration"),
            vec![parameter, rest],
            vec![CaptureDescriptor::new(capture_name.clone(), None)],
            None,
        )
        .unwrap();
        let capture = CapturedBinding::new(BindingCell::uninitialized(capture_name), environment);
        let policy = |kind| JavascriptFunctionPolicy {
            kind,
            constructable: kind == JavascriptFunctionKind::Function,
            defaults: BTreeMap::from([(
                Symbol::new("head"),
                JavascriptValue::String("default".into()),
            )]),
            asynchronous: true,
            generator: true,
            realm: Symbol::new("realm:fixture"),
            error_origin: "fixture.js:12".into(),
        };
        let ordinary_metadata = JavascriptFunction::new(
            plan.clone(),
            vec![capture.clone()],
            policy(JavascriptFunctionKind::Function),
            vec![],
        )
        .unwrap();
        let arrow_metadata = JavascriptFunction::new(
            plan,
            vec![capture],
            policy(JavascriptFunctionKind::Arrow),
            vec![],
        )
        .unwrap();
        assert_eq!(ordinary_metadata.plan(), arrow_metadata.plan());
        assert_eq!(
            ordinary_metadata.captures()[0].cell().name(),
            arrow_metadata.captures()[0].cell().name()
        );
        assert_eq!(
            ordinary_metadata.bind_arguments(&[]).unwrap(),
            BTreeMap::from([
                (
                    Symbol::new("head"),
                    vec![JavascriptValue::String("default".into())],
                ),
                (Symbol::new("tail"), vec![]),
            ])
        );

        let ordinary = m.function(ordinary_metadata, None).unwrap();
        let arrow = m
            .function(
                arrow_metadata,
                Some(JavascriptValue::String("lexical".into())),
            )
            .unwrap();
        let receiver = JavascriptValue::String("dynamic".into());
        assert_eq!(
            m.call_this(ordinary, receiver.clone()).unwrap(),
            JavascriptThis::Dynamic(receiver)
        );
        assert_eq!(
            m.call_this(arrow, JavascriptValue::Undefined).unwrap(),
            JavascriptThis::Lexical(JavascriptValue::String("lexical".into()))
        );
        assert!(m.construct(ordinary).is_ok());
        assert_eq!(
            m.construct(arrow),
            Err(JavascriptObjectError::NotConstructor)
        );
        assert!(!m.prototypes.contains_key(&ordinary.id()));
        assert!(!m.prototypes.contains_key(&arrow.id()));
    }
}
