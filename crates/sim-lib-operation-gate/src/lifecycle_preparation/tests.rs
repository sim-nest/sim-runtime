//! Existing owner codec laws; decoded DATA never supplies live authority.
use super::*;
use crate::LifecycleReservation;

fn preparation(reserved: bool) -> LifecyclePreparation {
    let dispatch = FencedDispatchId(
        Datum::String("original dispatch".into())
            .content_id()
            .unwrap(),
    );
    let binding = node(
        "test-only-binding",
        vec![
            ("resource", Datum::String("original resource".into())),
            (
                "capture",
                Datum::Vector(vec![Datum::Bool(false), Datum::String("data only".into())]),
            ),
        ],
    );
    let value = LifecyclePreparation::new(dispatch.clone(), binding).unwrap();
    if reserved {
        let reservation =
            LifecycleReservation::new(dispatch, Datum::String("original destination".into()))
                .unwrap();
        value.in_reservation(Some(&reservation)).unwrap()
    } else {
        value
    }
}

fn member_mut<'a>(value: &'a mut Datum, name: &str) -> &'a mut Datum {
    let Datum::Node { fields, .. } = value else {
        panic!("node")
    };
    &mut fields
        .iter_mut()
        .find(|(key, _)| *key == Symbol::new(name))
        .unwrap()
        .1
}

fn reverse_fields(value: &mut Datum) {
    if let Datum::Node { fields, .. } = value {
        fields.reverse();
        for (_, value) in fields {
            reverse_fields(value);
        }
    }
}

fn assert_noncanonical(value: &Datum) {
    assert!(
        matches!(
            LifecyclePreparation::from_datum(value),
            Err(OperationError::NonCanonical(_))
        ),
        "{value:?}"
    );
}

#[test]
fn reserved_and_legacy_data_roundtrip_through_existing_codec_without_new_identity() {
    for reserved in [false, true] {
        let original = preparation(reserved);
        let datum = original.canonical_datum();
        let decoded = LifecyclePreparation::from_datum(&datum).unwrap();
        assert_eq!(decoded, original);
        assert_eq!(decoded.id().content_id(), &datum.content_id().unwrap());
        assert_eq!(decoded.reservation().is_some(), reserved);
        assert_eq!(decoded.dispatch(), original.dispatch());
        assert_eq!(decoded.binding(), original.binding());
        assert_eq!(decoded.canonical_datum(), datum);
    }
}

#[test]
fn recursive_field_reordering_keeps_identity_and_leaves_supplied_data_unchanged() {
    for reserved in [false, true] {
        let original = preparation(reserved);
        let mut reordered = original.canonical_datum();
        reverse_fields(&mut reordered);
        let retained = reordered.clone();
        let decoded = LifecyclePreparation::from_datum(&reordered).unwrap();
        assert_eq!(reordered, retained);
        assert_eq!(decoded.id(), original.id());
        assert_eq!(decoded.dispatch(), original.dispatch());
        assert_eq!(decoded.reservation(), original.reservation());
        assert_eq!(
            decoded.binding().canonical_bytes().unwrap(),
            original.binding().canonical_bytes().unwrap()
        );
    }
}

#[test]
fn malformed_schema_and_duplicate_fields_cannot_decode_or_downgrade() {
    for reserved in [false, true] {
        for defect in [
            "not-node",
            "unknown-tag",
            "alias-tag",
            "missing",
            "extra",
            "duplicate",
            "unknown-field",
            "wrong-version",
        ] {
            let mut datum = preparation(reserved).canonical_datum();
            if defect == "not-node" {
                datum = Datum::Nil;
            } else {
                let Datum::Node { tag, fields } = &mut datum else {
                    panic!("node")
                };
                match defect {
                    "unknown-tag" => {
                        *tag = Symbol::qualified("operation", "lifecycle-preparation-v2")
                    }
                    "alias-tag" => *tag = Symbol::new(tag.as_qualified_str()),
                    "missing" => {
                        fields.pop();
                    }
                    "extra" => fields.push((Symbol::new("authority"), Datum::Bool(true))),
                    "duplicate" => fields[1] = fields[0].clone(),
                    "unknown-field" => fields.last_mut().unwrap().0 = Symbol::new("foreign"),
                    _ => {
                        *tag = Symbol::qualified(
                            "operation",
                            if reserved {
                                "lifecycle-preparation-v1"
                            } else {
                                "lifecycle-reserved-preparation-v1"
                            },
                        )
                    }
                }
            }
            assert_noncanonical(&datum);
        }
    }
}

#[test]
fn malformed_content_ids_and_noncanonical_bindings_fail_the_real_decoder() {
    for name in ["dispatch", "reservation"] {
        for defect in ["not-id", "short", "long", "algorithm", "duplicate"] {
            let mut datum = preparation(true).canonical_datum();
            let id = member_mut(&mut datum, name);
            match defect {
                "not-id" => *id = Datum::Nil,
                "short" => *member_mut(id, "digest") = Datum::Bytes(vec![1; 31]),
                "long" => *member_mut(id, "digest") = Datum::Bytes(vec![1; 33]),
                "algorithm" => *member_mut(id, "algorithm") = Datum::String("sha256".into()),
                _ => {
                    let Datum::Node { fields, .. } = id else {
                        panic!("node")
                    };
                    fields[1] = fields[0].clone();
                }
            }
            assert_noncanonical(&datum);
        }
    }
    for reserved in [false, true] {
        let mut datum = preparation(reserved).canonical_datum();
        *member_mut(&mut datum, "binding") = Datum::Set(vec![Datum::Nil, Datum::Nil]);
        assert!(matches!(
            LifecyclePreparation::from_datum(&datum),
            Err(OperationError::NonCanonical("semantic datum"))
        ));
    }
}

#[test]
fn valid_substitution_derives_a_different_identity_instead_of_authenticating_original() {
    let original = preparation(true);
    for name in ["dispatch", "reservation", "binding"] {
        let mut datum = original.canonical_datum();
        let changed = member_mut(&mut datum, name);
        if name == "binding" {
            *changed = Datum::String("different resource data".into());
        } else {
            let Datum::Bytes(bytes) = member_mut(changed, "digest") else {
                panic!("digest")
            };
            bytes[0] ^= 1;
        }
        let decoded = LifecyclePreparation::from_datum(&datum).unwrap();
        assert_ne!(decoded.id(), original.id(), "{name}");
        assert_eq!(decoded.id().content_id(), &datum.content_id().unwrap());
        // Representationally valid substitution is DATA, not a decoder error.
        // The original owner's exact identity/custody join must reject its use.
        assert_ne!(
            decoded.canonical_datum().content_id().unwrap(),
            original.canonical_datum().content_id().unwrap()
        );
    }
}
