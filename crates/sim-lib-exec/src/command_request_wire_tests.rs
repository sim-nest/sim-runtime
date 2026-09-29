//! Request decoding does not import executable or capability authority.

use super::*;

fn request() -> LocalCheckRequest {
    LocalCheckRequest::new(
        PacketRef::new("packet/exact").unwrap(),
        CommandId(
            Datum::String("installed-command".into())
                .content_id()
                .unwrap(),
        ),
        BuildSourceRef::new("source/sealed").unwrap(),
        CapabilityGrantRef::new("grant/scoped").unwrap(),
    )
}

fn entries(value: &mut Datum) -> &mut Vec<(Symbol, Datum)> {
    let Datum::Node { fields, .. } = value else {
        panic!("expected node");
    };
    fields
}

fn entry<'a>(value: &'a mut Datum, name: &str) -> &'a mut Datum {
    &mut entries(value)
        .iter_mut()
        .find(|(key, _)| key == &Symbol::new(name))
        .unwrap()
        .1
}

fn permute(value: &mut Datum) {
    if let Datum::Node { fields, .. } = value {
        fields.reverse();
        for (_, value) in fields {
            permute(value);
        }
    }
}

#[test]
fn canonical_and_permuted_requests_decode_to_the_same_exact_references() {
    for expected in [
        request(),
        request().with_network_grant(
            CapabilityName::new("network/declared"),
            CapabilityGrantRef::new("grant/network").unwrap(),
        ),
    ] {
        let mut value = expected.canonical_datum();
        assert_eq!(LocalCheckRequest::from_datum(&value).unwrap(), expected);
        permute(&mut value);
        assert_eq!(LocalCheckRequest::from_datum(&value).unwrap(), expected);
        assert_eq!(
            value.content_id().unwrap(),
            expected.canonical_datum().content_id().unwrap()
        );
    }
}

#[test]
fn strict_request_shape_refuses_payload_policy_and_duplicate_fields() {
    let original = request().canonical_datum();
    for replacement in ["argv", "program", "resources", "lease", "packet"] {
        let mut value = original.clone();
        entries(&mut value)[4].0 = Symbol::new(replacement);
        assert!(LocalCheckRequest::from_datum(&value).is_err());
    }
    for index in 0..5 {
        let mut value = original.clone();
        entries(&mut value).remove(index);
        assert!(LocalCheckRequest::from_datum(&value).is_err());
        let mut value = original.clone();
        let extra = entries(&mut value)[index].clone();
        entries(&mut value).push(extra);
        assert!(LocalCheckRequest::from_datum(&value).is_err());
    }
    let mut value = original;
    let Datum::Node { tag, .. } = &mut value else {
        unreachable!()
    };
    *tag = Symbol::qualified("foreign", "local-check-request-v1");
    assert!(LocalCheckRequest::from_datum(&value).is_err());
}

#[test]
fn malformed_references_and_content_ids_are_refused_without_normalization() {
    for name in ["packet", "source", "grant"] {
        for invalid in [
            Datum::Nil,
            Datum::String(String::new()),
            Datum::String("bad\0ref".into()),
        ] {
            let mut value = request().canonical_datum();
            *entry(&mut value, name) = invalid;
            assert!(LocalCheckRequest::from_datum(&value).is_err());
        }
    }
    for invalid in [
        Datum::Bytes(vec![0; 31]),
        Datum::Bytes(vec![0; 33]),
        Datum::String("00".into()),
    ] {
        let mut value = request().canonical_datum();
        *entry(entry(&mut value, "command"), "digest") = invalid;
        assert!(LocalCheckRequest::from_datum(&value).is_err());
    }
    let mut value = request().canonical_datum();
    *entry(entry(&mut value, "command"), "algorithm") =
        Datum::String("core/sha256-datum-v1".into());
    assert!(LocalCheckRequest::from_datum(&value).is_err());
    let mut value = request().canonical_datum();
    entries(entry(&mut value, "command"))[1].0 = Symbol::new("algorithm");
    assert!(LocalCheckRequest::from_datum(&value).is_err());
}

#[test]
fn network_authority_is_explicit_and_exact() {
    let network = request().with_network_grant(
        CapabilityName::new("network/declared"),
        CapabilityGrantRef::new("grant/network").unwrap(),
    );
    for name in ["capability", "grant"] {
        for invalid in [
            Datum::Nil,
            Datum::String(String::new()),
            Datum::String("bad\0ref".into()),
        ] {
            let mut value = network.canonical_datum();
            *entry(entry(&mut value, "network-grant"), name) = invalid;
            assert!(LocalCheckRequest::from_datum(&value).is_err());
        }
    }
    let mut value = network.canonical_datum();
    entries(entry(&mut value, "network-grant"))[1].0 = Symbol::new("capability");
    assert!(LocalCheckRequest::from_datum(&value).is_err());
    let mut changed = request().canonical_datum();
    *entry(entry(&mut changed, "command"), "digest") = Datum::Bytes(vec![9; 32]);
    let decoded = LocalCheckRequest::from_datum(&changed).unwrap();
    assert_ne!(decoded.command(), request().command());
    assert_eq!(decoded.canonical_datum(), changed);
}

#[test]
fn qualified_field_names_and_foreign_algorithms_never_alias_installed_references() {
    let mut value = request().canonical_datum();
    entries(&mut value)[0].0 = Symbol::qualified("foreign", "packet");
    assert!(LocalCheckRequest::from_datum(&value).is_err());
    let mut value = request().canonical_datum();
    *entry(entry(&mut value, "command"), "algorithm") =
        Datum::Symbol(Symbol::qualified("foreign", "digest"));
    let decoded = LocalCheckRequest::from_datum(&value).unwrap();
    assert_eq!(
        decoded.command().content_id().bytes,
        request().command().content_id().bytes
    );
    assert_ne!(decoded.command(), request().command());
}
