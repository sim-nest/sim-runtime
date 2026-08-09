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

/// Directly interpreted Python function with captured lexical values.
#[derive(Clone, Debug, PartialEq)]
pub struct PythonFunction {
    /// Parameters in declaration order.
    pub params: Vec<String>,
    /// Token body retained for direct evaluation.
    pub body: Vec<String>,
    /// Captured bindings.
    pub captures: BTreeMap<String, PythonValue>,
    /// Retained annotations.
    pub annotations: BTreeMap<String, Annotation>,
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
    /// Direct Python function.
    Function(PythonFunction),
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
        let tokens = lowered_tokens(lowered)?;
        let mut parser = Parser {
            tokens: &tokens,
            at: 0,
            steps: self.max_steps,
            env,
        };
        parser.module()
    }
}

fn lowered_tokens(expr: &Expr) -> Result<Vec<String>> {
    fn walk(expr: &Expr, out: &mut Vec<String>) -> Result<()> {
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
        if head.name.as_ref() == "token" {
            if let Some(Expr::String(text)) = args.get(1) {
                out.push(text.clone());
                return Ok(());
            }
            return Err(Error::Eval("malformed python token".into()));
        }
        for arg in args {
            walk(arg, out)?;
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(expr, &mut out)?;
    Ok(out.into_iter().filter(|t| !t.trim().is_empty()).collect())
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
            _ if token.starts_with(['\'', '"']) => {
                Ok(PythonValue::String(token[1..token.len() - 1].to_owned()))
            }
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
    fn annotations_remain_values_and_browse_metadata() {
        let annotation = Annotation {
            value: "list[int]".into(),
            browse: Some("example.py:1:10".into()),
        };
        assert_eq!(annotation.value, "list[int]");
        assert_eq!(annotation.browse.as_deref(), Some("example.py:1:10"));
    }
}
