use sim_kernel::{Error, Expr, Result};
use sim_lib_standard_core::LanguageProfile;
use std::collections::BTreeMap;

/// Values in the phase's scalar JavaScript core.
#[derive(Clone, Debug, PartialEq)]
pub enum JavascriptValue {
    /// `undefined`.
    Undefined,
    /// `null`.
    Null,
    /// Boolean.
    Bool(bool),
    /// Installed Number-domain value.
    Number(f64),
    /// Installed BigInt-domain value, bounded by its installed representation.
    BigInt(i64),
    /// String scalar.
    String(String),
    /// Cyclic identity from the managed arena.
    Managed(sim_lib_mutation::ManagedHandle),
}
/// Abrupt or normal completion produced directly by statements.
#[derive(Clone, Debug, PartialEq)]
pub enum Completion {
    /// Normal completion.
    Normal(JavascriptValue),
    /// `break`.
    Break,
    /// `continue`.
    Continue,
    /// `return`.
    Return(JavascriptValue),
    /// `throw`.
    Throw(JavascriptValue),
}
/// Only the lexical and variable bindings required by checked single-agent clauses.
#[derive(Clone, Debug, Default)]
pub struct JavascriptState {
    bindings: BTreeMap<String, JavascriptValue>,
}
impl JavascriptState {
    /// Read a binding.
    pub fn get(&self, name: &str) -> Option<&JavascriptValue> {
        self.bindings.get(name)
    }
    /// Define or assign a binding.
    pub fn set(&mut self, name: impl Into<String>, value: JavascriptValue) {
        self.bindings.insert(name.into(), value);
    }
}
/// Bounded direct evaluator over stable `javascript/*` lowering.
#[derive(Clone, Debug)]
pub struct JavascriptEvalPolicy {
    profile: LanguageProfile,
    max_steps: usize,
}
impl JavascriptEvalPolicy {
    /// Create the direct evaluator.
    pub fn new(max_steps: usize) -> Result<Self> {
        if max_steps == 0 {
            return Err(Error::Eval(
                "javascript direct evaluator requires a non-zero step bound".into(),
            ));
        }
        Ok(Self {
            profile: crate::javascript_core_profile(),
            max_steps,
        })
    }
    /// Selected profile.
    pub fn profile(&self) -> &LanguageProfile {
        &self.profile
    }
    /// Evaluate a codec-produced Script or Module lowering without an intermediate plan.
    pub fn eval_lowered(&self, lowered: &Expr, state: &mut JavascriptState) -> Result<Completion> {
        let tokens = lowered_tokens(lowered)?;
        Parser {
            tokens: &tokens,
            at: 0,
            steps: self.max_steps,
            state,
        }
        .program()
    }
}
fn lowered_tokens(expr: &Expr) -> Result<Vec<String>> {
    let Expr::Call { operator, args } = expr else {
        return Err(Error::Eval(
            "javascript evaluator accepts only codec/javascript lowered forms".into(),
        ));
    };
    let Expr::Symbol(head) = operator.as_ref() else {
        return Err(Error::Eval("malformed javascript lowering".into()));
    };
    if head.namespace.as_deref().map(AsRef::as_ref) != Some("javascript")
        || !matches!(head.name.as_ref(), "script" | "module")
    {
        return Err(Error::Eval(
            "javascript evaluator accepts only codec/javascript Script or Module forms".into(),
        ));
    }
    let mut out = Vec::new();
    for arg in args {
        if let Expr::Call { operator, args } = arg
            && matches!(operator.as_ref(), Expr::Symbol(s) if s.namespace.as_deref().map(AsRef::as_ref)==Some("javascript") && s.name.as_ref()=="token")
        {
            if let [Expr::Symbol(kind), Expr::String(text), Expr::Bool(_)] = args.as_slice() {
                if !matches!(kind.name.as_ref(), "trivia" | "end") {
                    out.push(text.clone());
                }
            } else {
                return Err(Error::Eval("malformed javascript token".into()));
            }
        }
    }
    Ok(out)
}
struct Parser<'a> {
    tokens: &'a [String],
    at: usize,
    steps: usize,
    state: &'a mut JavascriptState,
}
impl Parser<'_> {
    fn charge(&mut self) -> Result<()> {
        if self.steps == 0 {
            Err(Error::Eval(
                "javascript direct evaluation step bound exhausted".into(),
            ))
        } else {
            self.steps -= 1;
            Ok(())
        }
    }
    fn program(&mut self) -> Result<Completion> {
        let mut last = JavascriptValue::Undefined;
        while self.at < self.tokens.len() {
            self.charge()?;
            match self.statement()? {
                Completion::Normal(v) => last = v,
                abrupt => return Ok(abrupt),
            }
        }
        Ok(Completion::Normal(last))
    }
    fn statement(&mut self) -> Result<Completion> {
        if self.eat(";") {
            return Ok(Completion::Normal(JavascriptValue::Undefined));
        }
        if self.eat("{") {
            let mut last = JavascriptValue::Undefined;
            while !self.eat("}") {
                match self.statement()? {
                    Completion::Normal(v) => last = v,
                    c => return Ok(c),
                }
            }
            return Ok(Completion::Normal(last));
        }
        if matches!(self.peek(), Some("let" | "const" | "var")) {
            self.at += 1;
            let v = self.declaration()?;
            self.eat(";");
            return Ok(Completion::Normal(v));
        }
        if self.eat("if") {
            self.expect("(")?;
            let c = self.expr(0)?;
            self.expect(")")?;
            let yes_start = self.at;
            self.skip_statement()?;
            let yes_end = self.at;
            let no_range = if self.eat("else") {
                let start = self.at;
                self.skip_statement()?;
                Some((start, self.at))
            } else {
                None
            };
            let selected = if truthy(&c) {
                Some((yes_start, yes_end))
            } else {
                no_range
            };
            return if let Some((start, end)) = selected {
                let mut branch = Parser {
                    tokens: &self.tokens[start..end],
                    at: 0,
                    steps: self.steps,
                    state: self.state,
                };
                let completion = branch.statement()?;
                self.steps = branch.steps;
                Ok(completion)
            } else {
                Ok(Completion::Normal(JavascriptValue::Undefined))
            };
        }
        if self.eat("while") {
            return self.while_loop();
        }
        if self.eat("break") {
            self.eat(";");
            return Ok(Completion::Break);
        }
        if self.eat("continue") {
            self.eat(";");
            return Ok(Completion::Continue);
        }
        if self.eat("return") {
            let v = if self.peek() == Some(";") {
                JavascriptValue::Undefined
            } else {
                self.expr(0)?
            };
            self.eat(";");
            return Ok(Completion::Return(v));
        }
        if self.eat("throw") {
            let v = self.expr(0)?;
            self.eat(";");
            return Ok(Completion::Throw(v));
        }
        if self.at + 1 < self.tokens.len()
            && matches!(self.tokens[self.at + 1].as_str(), "=" | "+=" | "-=")
        {
            let name = self.tokens[self.at].clone();
            let op = self.tokens[self.at + 1].clone();
            self.at += 2;
            let rhs = self.expr(0)?;
            let value = if op == "=" {
                rhs
            } else {
                binary(
                    &op[..1],
                    self.state.get(&name).cloned().ok_or_else(|| {
                        Error::Eval(format!("javascript reference {name} is not defined"))
                    })?,
                    rhs,
                )?
            };
            self.state.set(name, value.clone());
            self.eat(";");
            return Ok(Completion::Normal(value));
        }
        let v = self.expr(0)?;
        self.eat(";");
        Ok(Completion::Normal(v))
    }
    fn declaration(&mut self) -> Result<JavascriptValue> {
        if self.eat("[") {
            let mut names = Vec::new();
            while !self.eat("]") {
                names.push(self.take()?);
                self.eat(",");
            }
            self.expect("=")?;
            self.expect("[")?;
            let mut values = Vec::new();
            while !self.eat("]") {
                values.push(self.expr(0)?);
                self.eat(",");
            }
            for (n, v) in names.into_iter().zip(values) {
                self.state.set(n, v);
            }
            return Ok(JavascriptValue::Undefined);
        }
        let name = self.take()?;
        let value = if self.eat("=") {
            self.expr(0)?
        } else {
            JavascriptValue::Undefined
        };
        self.state.set(name, value.clone());
        Ok(value)
    }
    fn while_loop(&mut self) -> Result<Completion> {
        self.expect("(")?;
        let cond_start = self.at;
        let mut depth = 1;
        while depth > 0 {
            match self.take()?.as_str() {
                "(" => depth += 1,
                ")" => depth -= 1,
                _ => {}
            }
        }
        let cond_end = self.at - 1;
        let body_start = self.at;
        self.skip_statement()?;
        let body_end = self.at;
        let mut last = JavascriptValue::Undefined;
        loop {
            let cond_tokens = &self.tokens[cond_start..cond_end];
            let cond = Parser {
                tokens: cond_tokens,
                at: 0,
                steps: self.steps,
                state: self.state,
            }
            .expr(0)?;
            if !truthy(&cond) {
                break;
            }
            let body_tokens = &self.tokens[body_start..body_end];
            let mut body = Parser {
                tokens: body_tokens,
                at: 0,
                steps: self.steps,
                state: self.state,
            };
            match body.statement()? {
                Completion::Normal(v) => last = v,
                Completion::Continue => {}
                Completion::Break => break,
                c => return Ok(c),
            }
            self.steps = body.steps;
        }
        Ok(Completion::Normal(last))
    }
    fn expr(&mut self, min: u8) -> Result<JavascriptValue> {
        self.charge()?;
        let mut left = self.unary()?;
        while let Some(op) = self.peek() {
            let (l, r) = match op {
                "||" => (1, 2),
                "&&" => (3, 4),
                "==" | "!=" | "===" | "!==" => (5, 6),
                "<" | "<=" | ">" | ">=" => (7, 8),
                "+" | "-" => (9, 10),
                "*" | "/" | "%" => (11, 12),
                _ => break,
            };
            if l < min {
                break;
            }
            let op = op.to_owned();
            self.at += 1;
            let right = self.expr(r)?;
            left = binary(&op, left, right)?;
        }
        Ok(left)
    }
    fn unary(&mut self) -> Result<JavascriptValue> {
        if self.eat("!") {
            return Ok(JavascriptValue::Bool(!truthy(&self.unary()?)));
        }
        if self.eat("-") {
            return match self.unary()? {
                JavascriptValue::Number(n) => Ok(JavascriptValue::Number(-n)),
                JavascriptValue::BigInt(n) => Ok(JavascriptValue::BigInt(-n)),
                v => Err(Error::Eval(format!("javascript unary - rejects {v:?}"))),
            };
        }
        self.atom()
    }
    fn atom(&mut self) -> Result<JavascriptValue> {
        let t = self.take()?;
        match t.as_str() {
            "undefined" => Ok(JavascriptValue::Undefined),
            "null" => Ok(JavascriptValue::Null),
            "true" => Ok(JavascriptValue::Bool(true)),
            "false" => Ok(JavascriptValue::Bool(false)),
            "(" => {
                let v = self.expr(0)?;
                self.expect(")")?;
                Ok(v)
            }
            _ if t.ends_with('n') && t[..t.len() - 1].bytes().all(|b| b.is_ascii_digit()) => t
                [..t.len() - 1]
                .parse()
                .map(JavascriptValue::BigInt)
                .map_err(|_| Error::Eval("javascript BigInt exceeds installed domain".into())),
            _ if t.as_bytes().first().is_some_and(u8::is_ascii_digit) => t
                .parse()
                .map(JavascriptValue::Number)
                .map_err(|_| Error::Eval(format!("invalid javascript Number {t}"))),
            _ if t.starts_with(['\'', '"']) => {
                Ok(JavascriptValue::String(t[1..t.len() - 1].to_owned()))
            }
            _ => self
                .state
                .get(&t)
                .cloned()
                .ok_or_else(|| Error::Eval(format!("javascript reference {t} is not defined"))),
        }
    }
    fn skip_statement(&mut self) -> Result<()> {
        if self.eat("{") {
            let mut d = 1;
            while d > 0 {
                match self.take()?.as_str() {
                    "{" => d += 1,
                    "}" => d -= 1,
                    _ => {}
                }
            }
        } else {
            while self.at < self.tokens.len() && !self.eat(";") {
                self.at += 1;
            }
        }
        Ok(())
    }
    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.at).map(String::as_str)
    }
    fn eat(&mut self, t: &str) -> bool {
        if self.peek() == Some(t) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, t: &str) -> Result<()> {
        if self.eat(t) {
            Ok(())
        } else {
            Err(Error::Eval(format!("javascript expected {t}")))
        }
    }
    fn take(&mut self) -> Result<String> {
        let t = self
            .tokens
            .get(self.at)
            .cloned()
            .ok_or_else(|| Error::Eval("javascript unexpected end of input".into()))?;
        self.at += 1;
        Ok(t)
    }
}
fn truthy(v: &JavascriptValue) -> bool {
    match v {
        JavascriptValue::Undefined | JavascriptValue::Null | JavascriptValue::Bool(false) => false,
        JavascriptValue::Number(n) => *n != 0.0 && !n.is_nan(),
        JavascriptValue::String(s) => !s.is_empty(),
        _ => true,
    }
}
fn to_number(v: &JavascriptValue) -> Result<f64> {
    match v {
        JavascriptValue::Number(n) => Ok(*n),
        JavascriptValue::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        JavascriptValue::Null => Ok(0.0),
        JavascriptValue::String(s) => s
            .parse()
            .map_err(|_| Error::Eval("javascript numeric coercion failed".into())),
        _ => Err(Error::Eval("javascript numeric coercion failed".into())),
    }
}
fn binary(op: &str, a: JavascriptValue, b: JavascriptValue) -> Result<JavascriptValue> {
    use JavascriptValue::*;
    match op {
        "&&" => Ok(if truthy(&a) { b } else { a }),
        "||" => Ok(if truthy(&a) { a } else { b }),
        "===" => Ok(Bool(a == b)),
        "!==" => Ok(Bool(a != b)),
        "==" | "!=" => {
            let eq = if a == b {
                true
            } else {
                to_number(&a).ok() == to_number(&b).ok()
            };
            Ok(Bool(if op == "==" { eq } else { !eq }))
        }
        "+" => match (a, b) {
            (String(a), b) => Ok(String(a + &display(&b))),
            (a, String(b)) => Ok(String(display(&a) + &b)),
            (BigInt(a), BigInt(b)) => a
                .checked_add(b)
                .map(BigInt)
                .ok_or_else(|| Error::Eval("javascript BigInt exceeds installed domain".into())),
            (a, b) => Ok(Number(to_number(&a)? + to_number(&b)?)),
        },
        "-" | "*" | "/" | "%" => {
            let (a, b) = (to_number(&a)?, to_number(&b)?);
            Ok(Number(match op {
                "-" => a - b,
                "*" => a * b,
                "/" => a / b,
                _ => a % b,
            }))
        }
        "<" | "<=" | ">" | ">=" => {
            let (a, b) = (to_number(&a)?, to_number(&b)?);
            Ok(Bool(match op {
                "<" => a < b,
                "<=" => a <= b,
                ">" => a > b,
                _ => a >= b,
            }))
        }
        _ => Err(Error::Eval(format!("unsupported javascript operator {op}"))),
    }
}
fn display(v: &JavascriptValue) -> String {
    match v {
        JavascriptValue::Undefined => "undefined".into(),
        JavascriptValue::Null => "null".into(),
        JavascriptValue::Bool(v) => v.to_string(),
        JavascriptValue::Number(v) => v.to_string(),
        JavascriptValue::BigInt(v) => v.to_string(),
        JavascriptValue::String(v) => v.clone(),
        JavascriptValue::Managed(_) => "[object Object]".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_kernel::Symbol;
    fn form(n: &str, a: Vec<Expr>) -> Expr {
        Expr::Call {
            operator: Box::new(Expr::Symbol(Symbol::qualified("javascript", n))),
            args: a,
        }
    }
    fn tok(k: &str, t: &str) -> Expr {
        form(
            "token",
            vec![
                Expr::Symbol(Symbol::new(k)),
                Expr::String(t.into()),
                Expr::Bool(true),
            ],
        )
    }
    fn script(ts: &[&str]) -> Expr {
        form(
            "script",
            ts.iter()
                .map(|t| {
                    tok(
                        if t.chars().next().unwrap_or('_').is_ascii_digit() {
                            "number"
                        } else {
                            "punctuator"
                        },
                        t,
                    )
                })
                .collect(),
        )
    }
    #[test]
    fn evaluates_declarations_coercion_and_loops_directly() {
        let e = script(&[
            "let", "x", "=", "0", ";", "while", "(", "x", "<", "4", ")", "{", "x", "+=", "1", ";",
            "}", "x", "===", "4", ";",
        ]);
        let mut s = JavascriptState::default();
        assert_eq!(
            JavascriptEvalPolicy::new(256)
                .unwrap()
                .eval_lowered(&e, &mut s)
                .unwrap(),
            Completion::Normal(JavascriptValue::Bool(true))
        );
    }
    #[test]
    fn destructuring_and_abrupt_completion_are_direct() {
        let e = script(&[
            "let", "[", "a", ",", "b", "]", "=", "[", "40", ",", "2", "]", ";", "return", "a", "+",
            "b", ";",
        ]);
        let mut s = JavascriptState::default();
        assert_eq!(
            JavascriptEvalPolicy::new(128)
                .unwrap()
                .eval_lowered(&e, &mut s)
                .unwrap(),
            Completion::Return(JavascriptValue::Number(42.0))
        );
    }
    #[test]
    fn conditional_executes_only_the_selected_statement() {
        let e = script(&[
            "let", "x", "=", "0", ";", "if", "(", "false", ")", "{", "x", "=", "1", ";", "}",
            "else", "{", "x", "=", "42", ";", "}", "x", ";",
        ]);
        let mut s = JavascriptState::default();
        assert_eq!(
            JavascriptEvalPolicy::new(128)
                .unwrap()
                .eval_lowered(&e, &mut s)
                .unwrap(),
            Completion::Normal(JavascriptValue::Number(42.0))
        );
    }
    #[test]
    fn rejects_non_codec_forms_and_bounds_steps() {
        let mut s = JavascriptState::default();
        assert!(
            JavascriptEvalPolicy::new(1)
                .unwrap()
                .eval_lowered(&script(&["1", "+", "2"]), &mut s)
                .is_err()
        );
        assert!(
            JavascriptEvalPolicy::new(8)
                .unwrap()
                .eval_lowered(&Expr::Bool(true), &mut s)
                .is_err()
        );
    }
}
