use sim_codec::{DecodeBudget, Input, ReadCx};
use sim_kernel::{
    CodecId, Error, Expr, LocatedExprTree, NumberLiteral, Origin, Result, SourceId, Span, Symbol,
};

/// Decodes EDN source into a [`LocatedExprTree`], interning source text and enforcing decode budgets.
///
/// Entry point used by [`ClojureEdnCodec`](crate::ClojureEdnCodec) to map surface
/// syntax onto the located [`Expr`] tree.
pub fn decode_clojure_edn_tree(
    cx: &mut ReadCx<'_>,
    source_id: impl Into<String>,
    input: Input,
) -> Result<LocatedExprTree> {
    let source = input_text(cx.codec, input)?;
    let mut budget = DecodeBudget::new(cx.limits);
    budget.check_input_bytes(cx.codec, source.len())?;
    let source_id = SourceId(source_id.into());
    cx.cx.sources_mut().intern_text(source_id.clone(), &source);
    let tree = parse_clojure_edn_source(cx.codec, source_id, &source, &mut budget)?;
    budget.check_tokens(cx.codec, tree_size(&tree))?;
    Ok(tree)
}

/// Parses a single top-level EDN value from source text into a [`LocatedExprTree`].
///
/// Fails closed if the input holds more than one top-level form. Spans are
/// resolved against the given [`SourceId`] and counted against the decode budget.
pub fn parse_clojure_edn_source(
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
    parser.skip_ws_commas_and_comments();
    if !parser.is_eof() {
        return parser.err("expected exactly one top-level EDN value");
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
        self.skip_ws_commas_and_comments();
        self.budget.enter_node(self.codec, depth)?;
        let start = self.index;
        let Some(byte) = self.peek() else {
            return self.err("expected EDN value");
        };
        match byte {
            b'(' => self.read_sequence(depth, start, b')', Expr::List),
            b'[' => self.read_sequence(depth, start, b']', Expr::Vector),
            b'{' => self.read_map(depth, start),
            b'"' => self.read_string(start),
            b'#' => self.read_dispatch(depth, start),
            b')' | b']' | b'}' => self.err("unexpected EDN delimiter"),
            _ => self.read_atom(start),
        }
    }

    fn read_sequence(
        &mut self,
        depth: usize,
        start: usize,
        close: u8,
        make_expr: fn(Vec<Expr>) -> Expr,
    ) -> Result<LocatedExprTree> {
        self.index += 1;
        let mut children = Vec::new();
        loop {
            self.skip_ws_commas_and_comments();
            match self.peek() {
                Some(byte) if byte == close => {
                    self.index += 1;
                    break;
                }
                Some(_) => children.push(self.read_expr(depth + 1)?),
                None => return self.err("unterminated EDN sequence"),
            }
        }
        self.budget
            .check_collection_len(self.codec, children.len())?;
        let expr = make_expr(children.iter().map(|child| child.expr.clone()).collect());
        Ok(self.tree(expr, start, self.index, children))
    }

    fn read_map(&mut self, depth: usize, start: usize) -> Result<LocatedExprTree> {
        self.index += 1;
        let mut children = Vec::new();
        let mut entries = Vec::new();
        loop {
            self.skip_ws_commas_and_comments();
            match self.peek() {
                Some(b'}') => {
                    self.index += 1;
                    break;
                }
                Some(_) => {
                    let key = self.read_expr(depth + 1)?;
                    self.skip_ws_commas_and_comments();
                    if self.peek() == Some(b'}') || self.is_eof() {
                        return self.err("EDN map expects an even number of forms");
                    }
                    let value = self.read_expr(depth + 1)?;
                    entries.push((key.expr.clone(), value.expr.clone()));
                    children.push(key);
                    children.push(value);
                }
                None => return self.err("unterminated EDN map"),
            }
        }
        self.budget
            .check_collection_len(self.codec, entries.len())?;
        Ok(self.tree(Expr::Map(entries), start, self.index, children))
    }

    fn read_dispatch(&mut self, depth: usize, start: usize) -> Result<LocatedExprTree> {
        self.index += 1;
        match self.peek() {
            Some(b'{') => self.read_set(depth, start),
            _ => self.err("unsupported EDN dispatch token"),
        }
    }

    fn read_set(&mut self, depth: usize, start: usize) -> Result<LocatedExprTree> {
        self.index += 1;
        let mut children = Vec::new();
        loop {
            self.skip_ws_commas_and_comments();
            match self.peek() {
                Some(b'}') => {
                    self.index += 1;
                    break;
                }
                Some(_) => children.push(self.read_expr(depth + 1)?),
                None => return self.err("unterminated EDN set"),
            }
        }
        self.budget
            .check_collection_len(self.codec, children.len())?;
        let expr = Expr::Set(children.iter().map(|child| child.expr.clone()).collect());
        Ok(self.tree(expr, start, self.index, children))
    }

    fn read_string(&mut self, start: usize) -> Result<LocatedExprTree> {
        self.index += 1;
        let mut out = String::new();
        while let Some(ch) = self.peek_char() {
            self.index += ch.len_utf8();
            match ch {
                '"' => {
                    self.budget.check_string_bytes(self.codec, out.len())?;
                    return Ok(self.tree(Expr::String(out), start, self.index, Vec::new()));
                }
                '\\' => out.push(self.read_escape()?),
                other => out.push(other),
            }
        }
        self.err("unterminated EDN string")
    }

    fn read_escape(&mut self) -> Result<char> {
        let Some(escaped) = self.peek_char() else {
            return self.err("unterminated EDN string escape");
        };
        self.index += escaped.len_utf8();
        Ok(match escaped {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '"' => '"',
            '\\' => '\\',
            other => other,
        })
    }

    fn read_atom(&mut self, start: usize) -> Result<LocatedExprTree> {
        let atom = self.take_atom();
        if atom.is_empty() {
            return self.err("expected EDN atom");
        }
        let expr = match atom.as_str() {
            "nil" => Expr::Nil,
            "true" => Expr::Bool(true),
            "false" => Expr::Bool(false),
            _ => number_literal(&atom)
                .map(Expr::Number)
                .unwrap_or_else(|| Expr::Symbol(symbol_atom(&atom))),
        };
        Ok(self.tree(expr, start, self.index, Vec::new()))
    }

    fn take_atom(&mut self) -> String {
        let start = self.index;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_whitespace()
                || matches!(
                    byte,
                    b',' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'"' | b';'
                )
            {
                break;
            }
            self.index += 1;
        }
        self.source[start..self.index].to_owned()
    }

    fn skip_ws_commas_and_comments(&mut self) {
        loop {
            while self
                .peek()
                .is_some_and(|byte| byte.is_ascii_whitespace() || byte == b',')
            {
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

    fn peek_char(&self) -> Option<char> {
        self.source[self.index..].chars().next()
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

fn symbol_atom(atom: &str) -> Symbol {
    if let Some(keyword) = atom.strip_prefix(':') {
        return Symbol::qualified("keyword", keyword.to_owned());
    }
    if let Some((namespace, name)) = atom.split_once('/') {
        return Symbol::qualified(namespace.to_owned(), name.to_owned());
    }
    Symbol::new(atom.to_owned())
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
            message: format!("EDN input is not valid UTF-8: {err}"),
        }),
    }
}
