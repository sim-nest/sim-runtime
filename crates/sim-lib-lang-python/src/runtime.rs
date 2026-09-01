use sim_kernel::{Error, Expr, Result};
use sim_lib_standard_core::LanguageProfile;
use std::collections::BTreeMap;

/// Retained annotation value and optional source/browse provenance.
#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    /// Unevaluated Python annotation spelling.
    pub value: String,
    /// Optional browse metadata supplied by the codec/host.
    pub browse: Option<String>,
}

/// Values in the declared Python scalar/container core.
#[derive(Clone, Debug, PartialEq)]
pub enum PythonValue {
    /// Python `None`.
    None,
    /// Boolean.
    Bool(bool),
    /// Arbitrary core integer spelling, composed through installed number policy.
    Int(i128),
    /// Finite float.
    Float(f64),
    /// Unicode string.
    String(String),
    /// Mutable/cyclic arena identity.
    Managed(sim_lib_mutation::ManagedHandle),
}

/// Thin direct evaluator policy. Its profile evidence proves the codec entry and organ set.
#[derive(Clone, Debug)]
pub struct PythonEvalPolicy {
    profile: LanguageProfile,
    max_steps: usize,
}
impl PythonEvalPolicy {
    /// Create a bounded direct evaluator.
    pub fn new(max_steps: usize) -> Result<Self> {
        if max_steps == 0 {
            return Err(Error::Eval(
                "python direct evaluator requires a non-zero step bound".into(),
            ));
        }
        Ok(Self {
            profile: crate::python_core_profile(),
            max_steps,
        })
    }
    /// Profile selected by this evaluator.
    pub fn profile(&self) -> &LanguageProfile {
        &self.profile
    }
    /// Evaluate one stable `codec/python` lowering. No compiled plan is created.
    pub fn eval_lowered(
        &self,
        lowered: &Expr,
        env: &mut BTreeMap<String, PythonValue>,
    ) -> Result<PythonValue> {
        let tokens = lowered_tokens(lowered, self.max_steps)?;
        let mut candidate = env.clone();
        let mut parser = Parser {
            tokens: &tokens,
            at: 0,
            steps: self.max_steps,
            env: &mut candidate,
        };
        let value = parser.module()?;
        *env = candidate;
        Ok(value)
    }
}

fn lowered_tokens(expr: &Expr, max_nodes: usize) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut pending = vec![(expr, true)];
    let mut visited = 0usize;
    while let Some((expr, root)) = pending.pop() {
        visited = visited
            .checked_add(1)
            .ok_or_else(|| Error::Eval("python lowering exceeds structural bound".into()))?;
        if visited > max_nodes {
            return Err(Error::Eval(
                "python lowering exceeds structural bound".into(),
            ));
        }
        let Expr::Call { operator, args } = expr else {
            return Err(Error::Eval(
                "python evaluator accepts only codec/python lowered forms".into(),
            ));
        };
        let Expr::Symbol(head) = operator.as_ref() else {
            return Err(Error::Eval("malformed python lowering".into()));
        };
        if head.namespace.as_deref().map(AsRef::as_ref) != Some("python") {
            return Err(Error::Eval(
                "python evaluator accepts only codec/python lowered forms".into(),
            ));
        }
        let name = head.name.as_ref();
        if root && name != "module" {
            return Err(Error::Eval(
                "python lowering root must be python/module".into(),
            ));
        }
        if name == "token" {
            let [
                Expr::Symbol(kind),
                Expr::String(text),
                Expr::Bool(executable),
            ] = args.as_slice()
            else {
                return Err(Error::Eval("malformed python token".into()));
            };
            if kind.namespace.is_some() {
                return Err(Error::Eval("malformed python token kind".into()));
            }
            let synthetic = matches!(kind.name.as_ref(), "dedent" | "end");
            let trivia = matches!(kind.name.as_ref(), "newline" | "indent" | "trivia");
            let executable_kind = matches!(
                kind.name.as_ref(),
                "name" | "keyword" | "number" | "string" | "operator"
            );
            let known = executable_kind
                || trivia
                || synthetic
                || matches!(kind.name.as_ref(), "f-string" | "template-string");
            if !known
                || *executable
                    != matches!(
                        kind.name.as_ref(),
                        "name" | "number" | "string" | "operator"
                    )
                || (text.is_empty() && !synthetic)
                || (synthetic && !text.is_empty())
            {
                return Err(Error::Eval("invalid python token metadata".into()));
            }
            if (executable_kind && *executable) || kind.name.as_ref() == "keyword" {
                out.push(text.clone());
            }
            continue;
        }
        if !matches!(
            name,
            "module" | "statement" | "suite" | "group" | "expression"
        ) {
            return Err(Error::Eval(format!("unknown python lowering node {name}")));
        }
        pending.extend(args.iter().rev().map(|child| (child, false)));
    }
    Ok(out)
}

struct Parser<'a> {
    tokens: &'a [String],
    at: usize,
    steps: usize,
    env: &'a mut BTreeMap<String, PythonValue>,
}
impl Parser<'_> {
    fn charge(&mut self) -> Result<()> {
        if self.steps == 0 {
            Err(Error::Eval(
                "python direct evaluation step bound exhausted".into(),
            ))
        } else {
            self.steps -= 1;
            Ok(())
        }
    }
    fn module(&mut self) -> Result<PythonValue> {
        let mut last = PythonValue::None;
        while self.at < self.tokens.len() {
            self.charge()?;
            last = self.statement()?;
            self.eat(";");
        }
        Ok(last)
    }
    fn statement(&mut self) -> Result<PythonValue> {
        if self.peek() == Some("pass") {
            self.at += 1;
            return Ok(PythonValue::None);
        }
        if self.at + 1 < self.tokens.len() && self.tokens[self.at + 1] == "=" {
            let name = self.tokens[self.at].clone();
            self.at += 2;
            let value = self.expr(0)?;
            self.env.insert(name, value.clone());
            return Ok(value);
        }
        self.expr(0)
    }
    fn expr(&mut self, min: u8) -> Result<PythonValue> {
        self.charge()?;
        let mut left = self.atom()?;
        while let Some(op) = self.peek().map(str::to_owned) {
            let (bp, right_bp) = match op.as_str() {
                "or" => (1, 2),
                "and" => (3, 4),
                "==" | "!=" | "<" | "<=" | ">" | ">=" => (5, 6),
                "+" | "-" => (7, 8),
                "*" | "/" | "//" | "%" => (9, 10),
                _ => break,
            };
            if bp < min {
                break;
            }
            self.at += 1;
            let right = self.expr(right_bp)?;
            left = binary(&op, left, right)?;
        }
        Ok(left)
    }
    fn atom(&mut self) -> Result<PythonValue> {
        let token = self
            .tokens
            .get(self.at)
            .ok_or_else(|| Error::Eval("python expected expression".into()))?
            .clone();
        self.at += 1;
        match token.as_str() {
            "None" => Ok(PythonValue::None),
            "True" => Ok(PythonValue::Bool(true)),
            "False" => Ok(PythonValue::Bool(false)),
            "(" => {
                let v = self.expr(0)?;
                if !self.eat(")") {
                    return Err(Error::Eval("python expected ')'".into()));
                }
                Ok(v)
            }
            _ if token.starts_with(['\'', '"']) => quoted_python_string(&token),
            _ if token.contains('.') => token
                .parse()
                .map(PythonValue::Float)
                .map_err(|_| Error::Eval(format!("invalid python float {token}"))),
            _ if token.as_bytes().first().is_some_and(u8::is_ascii_digit) => token
                .parse()
                .map(PythonValue::Int)
                .map_err(|_| Error::Eval(format!("invalid python integer {token}"))),
            _ => self
                .env
                .get(&token)
                .cloned()
                .ok_or_else(|| Error::Eval(format!("python name {token} is not defined"))),
        }
    }
    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.at).map(String::as_str)
    }
    fn eat(&mut self, token: &str) -> bool {
        if self.peek() == Some(token) {
            self.at += 1;
            true
        } else {
            false
        }
    }
}

fn quoted_python_string(token: &str) -> Result<PythonValue> {
    let quote = token.as_bytes()[0];
    if token.len() < 2 || token.as_bytes().last() != Some(&quote) {
        return Err(Error::Eval("malformed python string literal".into()));
    }
    Ok(PythonValue::String(token[1..token.len() - 1].to_owned()))
}
fn truth(v: &PythonValue) -> bool {
    match v {
        PythonValue::None | PythonValue::Bool(false) | PythonValue::Int(0) => false,
        PythonValue::Float(value) => *value != 0.0,
        PythonValue::String(value) => !value.is_empty(),
        _ => true,
    }
}
fn binary(op: &str, a: PythonValue, b: PythonValue) -> Result<PythonValue> {
    use PythonValue::*;
    match (op, a, b) {
        ("+", Int(a), Int(b)) => a
            .checked_add(b)
            .map(Int)
            .ok_or_else(|| Error::Eval("python integer bound exceeded".into())),
        ("-", Int(a), Int(b)) => a
            .checked_sub(b)
            .map(Int)
            .ok_or_else(|| Error::Eval("python integer bound exceeded".into())),
        ("*", Int(a), Int(b)) => a
            .checked_mul(b)
            .map(Int)
            .ok_or_else(|| Error::Eval("python integer bound exceeded".into())),
        ("//", Int(_), Int(0)) | ("%", Int(_), Int(0)) => {
            Err(Error::Eval("python integer division by zero".into()))
        }
        ("//", Int(a), Int(b)) => Ok(Int(a.div_euclid(b))),
        ("%", Int(a), Int(b)) => Ok(Int(a.rem_euclid(b))),
        ("/", Int(_), Int(0)) => Err(Error::Eval("python division by zero".into())),
        ("/", Int(a), Int(b)) => Ok(Float(a as f64 / b as f64)),
        ("+", String(a), String(b)) => Ok(String(a + &b)),
        ("and", a, b) => Ok(if truth(&a) { b } else { a }),
        ("or", a, b) => Ok(if truth(&a) { a } else { b }),
        ("==", a, b) => Ok(Bool(a == b)),
        ("!=", a, b) => Ok(Bool(a != b)),
        (op, a, b) => Err(Error::Eval(format!(
            "python operator {op} does not accept {a:?} and {b:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_kernel::Symbol;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    fn call(name: &str, args: Vec<Expr>) -> Expr {
        Expr::Call {
            operator: Box::new(Expr::Symbol(Symbol::qualified("python", name))),
            args,
        }
    }
    fn token(text: &str) -> Expr {
        call(
            "token",
            vec![
                Expr::Symbol(Symbol::new("name")),
                Expr::String(text.into()),
                Expr::Bool(true),
            ],
        )
    }
    #[test]
    fn evaluates_lowered_assignment_names_and_operators_directly() {
        let expr = call(
            "module",
            vec![call(
                "statement",
                vec![token("x"), token("="), token("40"), token("+"), token("2")],
            )],
        );
        let mut env = BTreeMap::new();
        assert_eq!(
            PythonEvalPolicy::new(64)
                .unwrap()
                .eval_lowered(&expr, &mut env)
                .unwrap(),
            PythonValue::Int(42)
        );
        assert_eq!(env["x"], PythonValue::Int(42));
    }
    #[test]
    fn rejects_non_codec_input_and_bounds_work() {
        let mut env = BTreeMap::new();
        assert!(
            PythonEvalPolicy::new(1)
                .unwrap()
                .eval_lowered(
                    &call(
                        "module",
                        vec![call("statement", vec![token("1"), token("+"), token("2")])]
                    ),
                    &mut env
                )
                .is_err()
        );
    }

    #[test]
    fn forged_lowerings_are_typed_refusals_and_never_panic() {
        let malformed = vec![
            Expr::Bool(true),
            call("statement", vec![token("1")]),
            call("module", vec![Expr::Bool(false)]),
            call("module", vec![call("alien", vec![])]),
            call("module", vec![call("token", vec![])]),
            call(
                "module",
                vec![call(
                    "token",
                    vec![
                        Expr::Symbol(Symbol::qualified("foreign", "name")),
                        Expr::String("x".into()),
                        Expr::Bool(true),
                    ],
                )],
            ),
            call("module", vec![token("")]),
            call("module", vec![token("'")]),
            call("module", vec![token("'mismatch\"")]),
            call("module", vec![token("1x")]),
        ];
        for bad in malformed {
            let result = catch_unwind(AssertUnwindSafe(|| {
                PythonEvalPolicy::new(16)
                    .unwrap()
                    .eval_lowered(&bad, &mut BTreeMap::new())
            }));
            assert!(
                matches!(result, Ok(Err(_))),
                "accepted or panicked: {bad:?}"
            );
        }

        let bad = call("module", vec![token("'")]);
        let result = catch_unwind(AssertUnwindSafe(|| {
            PythonEvalPolicy::new(16)
                .unwrap()
                .eval_lowered(&bad, &mut BTreeMap::new())
        }));
        assert!(matches!(result, Ok(Err(_))));

        let mut nested = token("1");
        for _ in 0..8 {
            nested = call("expression", vec![nested]);
        }
        let bounded = call("module", vec![nested]);
        assert_eq!(
            PythonEvalPolicy::new(16)
                .unwrap()
                .eval_lowered(&bounded, &mut BTreeMap::new())
                .unwrap(),
            PythonValue::Int(1)
        );
    }

    #[test]
    fn annotations_remain_values_and_browse_metadata() {
        let annotation = Annotation {
            value: "list[int]".into(),
            browse: Some("example.py:1:10".into()),
        };
        assert_eq!(annotation.value, "list[int]");
        assert_eq!(annotation.browse.as_deref(), Some("example.py:1:10"));
    }
}
