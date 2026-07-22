use sim_kernel::{Cx, Error, Expr, Result, Symbol, Value};

use crate::db::LogicDb;

/// Consults clauses from a table/dir-backed relative path.
///
/// Higher-level language surfaces use this helper so host reads stay on the
/// shared Table/Dir authority path.
pub fn consult_table_path(
    cx: &mut Cx,
    db: &mut LogicDb,
    source: &Value,
    path: &str,
) -> Result<usize> {
    let expr = read_table_path_expr(cx, source, path)?;
    consult_expr(db, expr)
}

pub(crate) fn consult_expr(db: &mut LogicDb, expr: Expr) -> Result<usize> {
    match expr {
        Expr::List(items) => {
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

fn read_table_path_expr(cx: &mut Cx, source: &Value, path: &str) -> Result<Expr> {
    let segments = relative_table_path(path)?;
    let (leaf, parents) = segments
        .split_last()
        .ok_or_else(|| Error::Eval("consult path must not be empty".to_owned()))?;
    let mut current = source.clone();
    for segment in parents {
        let dir = current.object().as_dir().ok_or_else(|| {
            Error::Eval(format!(
                "consult source does not expose directory segment {segment}"
            ))
        })?;
        current = dir
            .opendir(cx, Symbol::new(*segment))?
            .ok_or_else(|| Error::Eval(format!("consult source does not contain {path}")))?;
    }
    let table = current
        .object()
        .as_table_impl()
        .ok_or_else(|| Error::Eval("consult source does not implement Table".to_owned()))?;
    let key = Symbol::new(*leaf);
    if let Some(dir) = current.object().as_dir()
        && dir.is_dir(cx, key.clone())?
    {
        return Err(Error::Eval(format!(
            "consult source path {path} is a directory"
        )));
    }
    if !table.has(cx, key.clone())? {
        return Err(Error::Eval(format!(
            "consult source does not contain {path}"
        )));
    }
    table.get(cx, key)?.object().as_expr(cx)
}

fn relative_table_path(path: &str) -> Result<Vec<&str>> {
    if path.is_empty() || path.starts_with('/') || path.ends_with('/') || path.contains('\\') {
        return Err(Error::Eval(format!(
            "consult path must be a non-empty relative table path: {path}"
        )));
    }
    let segments: Vec<_> = path.split('/').collect();
    if segments
        .iter()
        .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return Err(Error::Eval(format!(
            "consult path must be a non-empty relative table path: {path}"
        )));
    }
    Ok(segments)
}
