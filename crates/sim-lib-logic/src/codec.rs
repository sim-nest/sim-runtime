use std::{fs, path::Path};

use sim_codec::{Input, decode_with_codec};
use sim_kernel::{Cx, ReadPolicy, Result, Symbol, logic_consult_file_capability};

use crate::{db::LogicDb, error::logic_eval_error};

pub(crate) fn consult_path(cx: &mut Cx, db: &mut LogicDb, path: &str) -> Result<usize> {
    cx.require(&logic_consult_file_capability())?;
    let bytes = fs::read(path).map_err(|err| logic_eval_error(err.to_string()))?;
    let codec = codec_for_path(path);
    let expr = decode_with_codec(
        cx,
        &codec,
        match codec.name.as_ref() {
            "binary" | "binary-base64" => Input::Bytes(bytes),
            _ => Input::Text(
                String::from_utf8(bytes).map_err(|err| logic_eval_error(err.to_string()))?,
            ),
        },
        ReadPolicy::default(),
    )?;
    consult_expr(db, expr)
}

pub(crate) fn consult_expr(db: &mut LogicDb, expr: sim_kernel::Expr) -> Result<usize> {
    match expr {
        sim_kernel::Expr::List(items) => {
            let mut count = 0usize;
            for item in items {
                db.assert_clause_expr(item)?;
                count += 1;
            }
            Ok(count)
        }
        other => {
            db.assert_clause_expr(other)?;
            Ok(1)
        }
    }
}

fn codec_for_path(path: &str) -> Symbol {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
    {
        "simlogicb64" | "simb64" => Symbol::qualified("codec", "binary-base64"),
        "json" => Symbol::qualified("codec", "json"),
        "alg" => Symbol::qualified("codec", "algol"),
        "slb8" => Symbol::qualified("codec", "binary"),
        _ => Symbol::qualified("codec", "lisp"),
    }
}
