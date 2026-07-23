// conformance: namespace organ imports, diagnostics, and Card projection

use sim_kernel::{
    Expr, Ref, Symbol,
    card::{card_for_ref, card_kind_predicate},
    force_list_to_vec,
    standard::standard_organ_kind,
};

use crate::*;

use sim_kernel::testing::bare_cx as cx;

fn source_namespace() -> Namespace {
    let mut source = Namespace::package(Symbol::qualified("pkg", "sequence"));
    source
        .define(Symbol::new("map"), Symbol::qualified("sequence", "map.v1"))
        .unwrap();
    source.export(Symbol::new("map")).unwrap();
    source
}

#[test]
fn rename_resolves_to_exported_target() {
    let source = source_namespace();
    let mut target = Namespace::module(Symbol::qualified("module", "user"));
    target
        .import_from(
            &source,
            &Symbol::new("map"),
            ImportOptions::new().rename(Symbol::new("seq-map")),
        )
        .unwrap();

    let resolved = target.resolve(&Symbol::new("seq-map")).unwrap();
    assert_eq!(resolved.name(), &Symbol::new("seq-map"));
    assert_eq!(resolved.target(), &Symbol::qualified("sequence", "map.v1"));
    assert_eq!(
        resolved.source(),
        &NamespaceBindingSource::Import {
            namespace: Symbol::qualified("pkg", "sequence"),
            exported: Symbol::new("map")
        }
    );
}

#[test]
fn shadow_conflicts_are_diagnostic() {
    let source = source_namespace();
    let mut target = Namespace::module(Symbol::qualified("module", "user"));
    target
        .define(Symbol::new("map"), Symbol::qualified("local", "map"))
        .unwrap();

    let err = target
        .import_from(&source, &Symbol::new("map"), ImportOptions::new())
        .unwrap_err();
    assert!(format!("{err}").contains("shadow conflict"));
    assert_eq!(target.diagnostics().len(), 1);
    assert_eq!(
        target.diagnostics()[0].code,
        Some(namespace_shadow_conflict_symbol())
    );
}

#[test]
fn package_and_module_kinds_are_explicit() {
    let package = Namespace::package(Symbol::qualified("pkg", "core"));
    let module = Namespace::module(Symbol::qualified("module", "core"));
    assert_eq!(package.kind(), NamespaceKind::Package);
    assert_eq!(module.kind(), NamespaceKind::Module);
}

#[test]
fn namespace_organ_claims_project_to_card() {
    let mut cx = cx();
    publish_namespace_organ_claims(&mut cx).unwrap();

    let claims = cx
        .query_facts(sim_kernel::ClaimPattern {
            subject: Some(Ref::Symbol(namespace_organ_symbol())),
            predicate: Some(card_kind_predicate()),
            object: Some(Ref::Symbol(standard_organ_kind())),
            include_revoked: false,
        })
        .unwrap();
    assert_eq!(claims.len(), 1);

    let card = card_for_ref(&mut cx, Ref::Symbol(namespace_organ_symbol())).unwrap();
    let table = card.object().as_table(&mut cx).unwrap();
    let entries = table.object().as_table_impl().unwrap();
    let ops = entries.get(&mut cx, Symbol::new("ops")).unwrap();
    let list = ops.object().as_list().unwrap();
    let values = force_list_to_vec(&mut cx, list, "namespace organ ops").unwrap();

    assert!(values.into_iter().any(|value| {
        value.object().as_expr(&mut cx).unwrap()
            == Expr::Symbol(Symbol::qualified("namespace", "rename.v1"))
    }));
}
