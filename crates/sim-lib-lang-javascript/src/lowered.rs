use sim_kernel::{Error, Expr, Result};

pub(crate) fn tokens(expr: &Expr, max_nodes: usize) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut pending = vec![(expr, true)];
    let mut visited = 0usize;
    while let Some((expr, root)) = pending.pop() {
        visited = visited
            .checked_add(1)
            .ok_or_else(|| Error::Eval("javascript lowering exceeds structural bound".into()))?;
        if visited > max_nodes {
            return Err(Error::Eval(
                "javascript lowering exceeds structural bound".into(),
            ));
        }
        let Expr::Call { operator, args } = expr else {
            return Err(Error::Eval(
                "javascript evaluator accepts only codec/javascript lowered forms".into(),
            ));
        };
        let Expr::Symbol(head) = operator.as_ref() else {
            return Err(Error::Eval("malformed javascript lowering".into()));
        };
        if head.namespace.as_deref().map(AsRef::as_ref) != Some("javascript") {
            return Err(Error::Eval(
                "javascript evaluator accepts only codec/javascript Script or Module forms".into(),
            ));
        }
        let name = head.name.as_ref();
        if root && !matches!(name, "script" | "module") {
            return Err(Error::Eval(
                "javascript lowering root must be script or module".into(),
            ));
        }
        if name == "token" {
            validate_token(args, &mut out)?;
            continue;
        }
        if !matches!(
            name,
            "script"
                | "module"
                | "statement-list"
                | "declaration"
                | "statement"
                | "function"
                | "class"
                | "import"
                | "export"
                | "expression"
                | "group"
        ) {
            return Err(Error::Eval(format!(
                "unknown javascript lowering node {name}"
            )));
        }
        pending.extend(args.iter().rev().map(|child| (child, false)));
    }
    Ok(out)
}

fn validate_token(args: &[Expr], out: &mut Vec<String>) -> Result<()> {
    let [
        Expr::Symbol(kind),
        Expr::String(text),
        Expr::Bool(executable),
    ] = args
    else {
        return Err(Error::Eval("malformed javascript token".into()));
    };
    if kind.namespace.is_some() {
        return Err(Error::Eval("malformed javascript token kind".into()));
    }
    let executable_kind = matches!(
        kind.name.as_ref(),
        "identifier" | "number" | "string" | "regexp" | "template" | "punctuator"
    );
    let keyword = kind.name.as_ref() == "keyword";
    let trivia = kind.name.as_ref() == "trivia";
    let end = kind.name.as_ref() == "end";
    if !(executable_kind || keyword || trivia || end)
        || *executable != executable_kind
        || (text.is_empty() && !end)
        || (end && !text.is_empty())
    {
        return Err(Error::Eval("invalid javascript token metadata".into()));
    }
    if *executable || keyword {
        out.push(text.clone());
    }
    Ok(())
}

pub(crate) fn quoted_string(token: &str) -> Result<String> {
    let quote = token.as_bytes()[0];
    if token.len() < 2 || token.as_bytes().last() != Some(&quote) {
        return Err(Error::Eval("malformed javascript string literal".into()));
    }
    Ok(token[1..token.len() - 1].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Completion, JavascriptEvalPolicy, JavascriptState, JavascriptValue};
    use sim_kernel::Symbol;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    fn form(name: &str, args: Vec<Expr>) -> Expr {
        Expr::Call {
            operator: Box::new(Expr::Symbol(Symbol::qualified("javascript", name))),
            args,
        }
    }

    fn token(kind: &str, text: &str) -> Expr {
        form(
            "token",
            vec![
                Expr::Symbol(Symbol::new(kind)),
                Expr::String(text.into()),
                Expr::Bool(true),
            ],
        )
    }

    #[test]
    fn forged_lowerings_are_typed_refusals_and_never_execute_a_prefix() {
        let malformed = vec![
            Expr::Bool(true),
            form("statement", vec![token("number", "1")]),
            form("script", vec![Expr::Bool(false)]),
            form("script", vec![form("alien", vec![])]),
            form("script", vec![form("token", vec![])]),
            form(
                "script",
                vec![form(
                    "token",
                    vec![
                        Expr::Symbol(Symbol::qualified("foreign", "number")),
                        Expr::String("1".into()),
                        Expr::Bool(true),
                    ],
                )],
            ),
            form("script", vec![token("number", "")]),
            form("script", vec![token("string", "'")]),
            form("script", vec![token("string", "'mismatch\"")]),
            form("script", vec![token("number", "1x")]),
        ];
        for bad in malformed {
            let mut state = JavascriptState::default();
            state.set("sentinel", JavascriptValue::Number(7.0));
            let result = catch_unwind(AssertUnwindSafe(|| {
                JavascriptEvalPolicy::new(16)
                    .unwrap()
                    .eval_lowered(&bad, &mut state)
            }));
            assert!(
                matches!(result, Ok(Err(_))),
                "accepted or panicked: {bad:?}"
            );
            assert_eq!(state.get("sentinel"), Some(&JavascriptValue::Number(7.0)));
        }

        let mut nested = token("number", "1");
        for _ in 0..8 {
            nested = form("expression", vec![nested]);
        }
        assert_eq!(
            JavascriptEvalPolicy::new(16)
                .unwrap()
                .eval_lowered(
                    &form("script", vec![nested]),
                    &mut JavascriptState::default()
                )
                .unwrap(),
            Completion::Normal(JavascriptValue::Number(1.0))
        );
    }
}
