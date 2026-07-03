use sim_codec::{DecodeBudget, Input, ReadCx};
use sim_kernel::{
    CodecId, Error, Expr, LocatedExprTree, NumberLiteral, Origin, Result, SourceId, Span, Symbol,
};

/// Decodes Scheme surface text into a located `Expr` tree under codec budgets.
///
/// Interns the source, enforces the decode limits, and returns the single
/// top-level form as a [`LocatedExprTree`].
pub fn decode_scheme_tree(
    cx: &mut ReadCx<'_>,
    source_id: impl Into<String>,
    input: Input,
) -> Result<LocatedExprTree> {
    let source = input_text(cx.codec, input)?;
    let mut budget = DecodeBudget::new(cx.limits);
    budget.check_input_bytes(cx.codec, source.len())?;
    let source_id = SourceId(source_id.into());
    cx.cx.sources_mut().intern_text(source_id.clone(), &source);
    let tree = parse_scheme_source(cx.codec, source_id, &source, &mut budget)?;
    budget.check_tokens(cx.codec, tree_size(&tree))?;
    Ok(tree)
}

/// Parses one top-level Scheme form from source text into a located `Expr` tree.
///
/// Lower-level entry point behind [`decode_scheme_tree`]; the caller supplies
/// the codec id, interned source id, and a [`DecodeBudget`]. Errors if the input
/// holds more than one top-level expression.
pub fn parse_scheme_source(
    codec: CodecId,
    source_id: SourceId,
    source: &str,
    budget: &mut DecodeBudget,
) -> Result<LocatedExprTree> {
    let mut parser = Parser {
        codec,
        source_id,
        source,
        bytes: source.as_bytes(),
        index: 0,
        budget,
    };
    let tree = parser.read_expr(0)?;
    parser.skip_ws_and_comments();
    if !parser.is_eof() {
        return parser.err("expected exactly one top-level expression");
    }
    Ok(tree)
}

struct Parser<'a, 'b> {
    codec: CodecId,
    source_id: SourceId,
    source: &'a str,
    bytes: &'a [u8],
    index: usize,
    budget: &'b mut DecodeBudget,
}

impl Parser<'_, '_> {
    fn read_expr(&mut self, depth: usize) -> Result<LocatedExprTree> {
        self.skip_ws_and_comments();
        self.budget.enter_node(self.codec, depth)?;
        let start = self.index;
        let Some(byte) = self.peek() else {
            return self.err("expected expression");
        };
        match byte {
            b'(' => self.read_list(depth, start),
            b')' => self.err("unexpected close parenthesis"),
            b'\'' => self.read_quote(depth, start),
            b'"' => self.read_string(start),
            b'#' => self.read_hash_atom(start),
            _ => self.read_atom(start),
        }
    }

    fn read_list(&mut self, depth: usize, start: usize) -> Result<LocatedExprTree> {
        self.index += 1;
        let mut children = Vec::new();
        loop {
            self.skip_ws_and_comments();
            match self.peek() {
                Some(b')') => {
                    self.index += 1;
                    break;
                }
                Some(_) => children.push(self.read_expr(depth + 1)?),
                None => return self.err("unterminated list"),
            }
        }
        self.budget
            .check_collection_len(self.codec, children.len())?;
        let expr = Expr::List(children.iter().map(|child| child.expr.clone()).collect());
        Ok(self.tree(expr, start, self.index, children))
    }

    fn read_quote(&mut self, depth: usize, start: usize) -> Result<LocatedExprTree> {
        self.index += 1;
        let quoted = self.read_expr(depth + 1)?;
        let quote = self.tree(
            Expr::Symbol(Symbol::new("quote")),
            start,
            start + 1,
            Vec::new(),
        );
        let end = quoted
            .origin
            .as_ref()
            .map(|origin| origin.span.end)
            .unwrap_or(self.index);
        Ok(self.tree(
            Expr::List(vec![quote.expr.clone(), quoted.expr.clone()]),
            start,
            end,
            vec![quote, quoted],
        ))
    }

    fn read_string(&mut self, start: usize) -> Result<LocatedExprTree> {
        self.index += 1;
        let mut out = String::new();
        while let Some(byte) = self.peek() {
            self.index += 1;
            match byte {
                b'"' => {
                    self.budget.check_string_bytes(self.codec, out.len())?;
                    return Ok(self.tree(Expr::String(out), start, self.index, Vec::new()));
                }
                b'\\' => {
                    let Some(escaped) = self.peek() else {
                        return self.err("unterminated string escape");
                    };
                    self.index += 1;
                    out.push(match escaped {
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'"' => '"',
                        b'\\' => '\\',
                        other => other as char,
                    });
                }
                other => out.push(other as char),
            }
        }
        self.err("unterminated string")
    }

    fn read_hash_atom(&mut self, start: usize) -> Result<LocatedExprTree> {
        let atom = self.take_atom();
        let expr = match atom.as_str() {
            "#t" | "#true" => Expr::Bool(true),
            "#f" | "#false" => Expr::Bool(false),
            _ => {
                return Err(Error::CodecError {
                    codec: self.codec,
                    message: format!("unsupported Scheme hash token {atom}"),
                });
            }
        };
        Ok(self.tree(expr, start, self.index, Vec::new()))
    }

    fn read_atom(&mut self, start: usize) -> Result<LocatedExprTree> {
        let atom = self.take_atom();
        if atom.is_empty() {
            return self.err("expected atom");
        }
        let expr = if let Some(number) = number_literal(&atom) {
            Expr::Number(number)
        } else {
            Expr::Symbol(Symbol::new(atom))
        };
        Ok(self.tree(expr, start, self.index, Vec::new()))
    }

    fn take_atom(&mut self) -> String {
        let start = self.index;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_whitespace() || matches!(byte, b'(' | b')' | b'"' | b';') {
                break;
            }
            self.index += 1;
        }
        self.source[start..self.index].to_owned()
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
                self.index += 1;
            }
            if self.peek() != Some(b';') {
                return;
            }
            while let Some(byte) = self.peek() {
                self.index += 1;
                if byte == b'\n' {
                    break;
                }
            }
        }
    }

    fn tree(
        &self,
        expr: Expr,
        start: usize,
        end: usize,
        children: Vec<LocatedExprTree>,
    ) -> LocatedExprTree {
        LocatedExprTree {
            expr,
            origin: Some(Origin {
                codec: self.codec,
                source: self.source_id.clone(),
                span: Span { start, end },
                trivia: Vec::new(),
            }),
            children,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn is_eof(&self) -> bool {
        self.index >= self.bytes.len()
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T> {
        Err(Error::CodecError {
            codec: self.codec,
            message: message.into(),
        })
    }
}

fn number_literal(raw: &str) -> Option<NumberLiteral> {
    let is_integer = raw
        .strip_prefix(['+', '-'])
        .unwrap_or(raw)
        .chars()
        .all(|ch| ch.is_ascii_digit());
    if !is_integer || raw == "+" || raw == "-" {
        return None;
    }
    Some(NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: raw.to_owned(),
    })
}

fn tree_size(tree: &LocatedExprTree) -> usize {
    1 + tree.children.iter().map(tree_size).sum::<usize>()
}

fn input_text(codec: CodecId, input: Input) -> Result<String> {
    match input {
        Input::Text(text) => Ok(text),
        Input::Bytes(bytes) => String::from_utf8(bytes).map_err(|err| Error::CodecError {
            codec,
            message: format!("Scheme input is not valid UTF-8: {err}"),
        }),
    }
}
