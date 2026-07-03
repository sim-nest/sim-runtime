use sim_kernel::{Export, ExportRecord, Symbol};

/// Returns the Prolog surface export records as declared manifest metadata.
pub fn prolog_exports() -> Vec<ExportRecord> {
    prolog_export_declarations()
        .into_iter()
        .map(|export| export.declared_record())
        .collect()
}

pub(crate) fn prolog_export_declarations() -> Vec<Export> {
    let mut exports = vec![
        Export::Value {
            symbol: Symbol::qualified("prolog", "db"),
        },
        Export::Value {
            symbol: Symbol::qualified("prolog", "config-state"),
        },
    ];
    for symbol in [
        Symbol::qualified("prolog", "assert!"),
        Symbol::qualified("prolog", "retract!"),
        Symbol::qualified("prolog", "query"),
        Symbol::qualified("prolog", "query/all"),
        Symbol::qualified("prolog", "query-seq"),
        Symbol::qualified("prolog", "consult"),
    ] {
        exports.push(Export::Function {
            symbol,
            function_id: None,
        });
    }
    exports
}
