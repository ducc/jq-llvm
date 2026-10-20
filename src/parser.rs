//! Lexer and recursive-descent / precedence-climbing parser for jq.

use crate::ast::*;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Field(String),
    Var(String),
    Num(f64),
    NumText(String),
    Str(Vec<SPart>),
    Format(String),
    Kw(&'static str),
    P(&'static str),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SPart {
    Lit(String),
    Interp(Vec<(Tok, usize)>),
}

const KEYWORDS: &[&str] = &[
    "def", "if", "then", "elif", "else", "end", "as", "reduce", "foreach", "try", "catch", "label", "import",
    "include", "and", "or", "__loc__",
];

// longest first
const PUNCT: &[&str] = &[
    "?//=", "?//", "//=", "|=", "+=", "-=", "*=", "/=", "%=", "==", "!=", "<=", ">=", "//", "..", ".", "[", "]", "{",
    "}", "(", ")", "|", ",", ":", ";", "=", "<", ">", "+", "-", "*", "/", "%", "?",
];

pub struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
    line: usize,
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}
fn is_ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

impl<'a> Lexer<'a> {
    pub fn new(s: &'a str) -> Self {
        Lexer { s: s.as_bytes(), i: 0, line: 1 }
    }

    fn err<T>(&self, msg: &str) -> Result<T, String> {
        Err(format!("syntax error: {} at line {}", msg, self.line))
    }

    fn skip_ws(&mut self) {
        while self.i < self.s.len() {
            let c = self.s[self.i];
            if c == b'\n' {
                self.line += 1;
                self.i += 1;
            } else if c.is_ascii_whitespace() {
                self.i += 1;
            } else if c == b'#' {
                while self.i < self.s.len() && self.s[self.i] != b'\n' {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
    }

    fn ident(&mut self) -> String {
        let st = self.i;
        while self.i < self.s.len() {
            let c = self.s[self.i];
            if is_ident_char(c) {
                self.i += 1;
            } else if c == b':' && self.i + 2 < self.s.len() && self.s[self.i + 1] == b':' && is_ident_start(self.s[self.i + 2]) {
                self.i += 2;
            } else {
                break;
            }
        }
        String::from_utf8_lossy(&self.s[st..self.i]).into_owned()
    }

    /// Lex tokens until EOF, or until an unbalanced ')' when `nested`.
    pub fn tokens(&mut self, nested: bool) -> Result<Vec<(Tok, usize)>, String> {
        let mut out = vec![];
        let mut depth = 0i32;
        loop {
            self.skip_ws();
            if self.i >= self.s.len() {
                if nested {
                    return self.err("unterminated string interpolation");
                }
                return Ok(out);
            }
            let line = self.line;
            let c = self.s[self.i];
            if c == b'"' {
                self.i += 1;
                let parts = self.string()?;
                out.push((Tok::Str(parts), line));
                continue;
            }
            if c == b'.' && self.i + 1 < self.s.len() && is_ident_start(self.s[self.i + 1]) {
                self.i += 1;
                let name = self.ident();
                out.push((Tok::Field(name), line));
                continue;
            }
            if c.is_ascii_digit() || (c == b'.' && self.i + 1 < self.s.len() && self.s[self.i + 1].is_ascii_digit()) {
                let st = self.i;
                while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                    self.i += 1;
                }
                if self.i < self.s.len() && self.s[self.i] == b'.' {
                    self.i += 1;
                    while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                        self.i += 1;
                    }
                }
                if self.i < self.s.len() && (self.s[self.i] == b'e' || self.s[self.i] == b'E') {
                    let save = self.i;
                    self.i += 1;
                    if self.i < self.s.len() && (self.s[self.i] == b'+' || self.s[self.i] == b'-') {
                        self.i += 1;
                    }
                    if self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                            self.i += 1;
                        }
                    } else {
                        self.i = save;
                    }
                }
                let txt = std::str::from_utf8(&self.s[st..self.i]).unwrap();
                let v: f64 = txt.parse().map_err(|_| format!("bad number {}", txt))?;
                let simple = txt.bytes().all(|b| b.is_ascii_digit()) && txt.len() <= 15;
                if simple {
                    out.push((Tok::Num(v), line));
                } else {
                    let t = if txt.starts_with('.') { format!("0{}", txt) } else { txt.to_string() };
                    out.push((Tok::NumText(t), line));
                }
                continue;
            }
            if c == b'$' {
                self.i += 1;
                if self.i < self.s.len() && is_ident_start(self.s[self.i]) {
                    let name = self.ident();
                    out.push((Tok::Var(name), line));
                    continue;
                }
                return self.err("expected variable name after $");
            }
            if c == b'@' {
                self.i += 1;
                let name = self.ident();
                out.push((Tok::Format(name), line));
                continue;
            }
            if is_ident_start(c) {
                let name = self.ident();
                if let Some(k) = KEYWORDS.iter().find(|k| **k == name) {
                    out.push((Tok::Kw(k), line));
                } else {
                    out.push((Tok::Ident(name), line));
                }
                continue;
            }
            let mut matched = None;
            for p in PUNCT {
                if self.s[self.i..].starts_with(p.as_bytes()) {
                    matched = Some(*p);
                    break;
                }
            }
            match matched {
                Some(p) => {
                    if nested {
                        if p == "(" {
                            depth += 1;
                        } else if p == ")" {
                            if depth == 0 {
                                self.i += 1;
                                return Ok(out);
                            }
                            depth -= 1;
                        }
                    }
                    self.i += p.len();
                    out.push((Tok::P(p), line));
                }
                None => return self.err(&format!("unexpected character '{}'", c as char)),
            }
        }
    }

    fn string(&mut self) -> Result<Vec<SPart>, String> {
        let mut parts = vec![];
        let mut cur: Vec<u8> = vec![];
        loop {
            if self.i >= self.s.len() {
                return self.err("unterminated string");
            }
            let c = self.s[self.i];
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    if self.i >= self.s.len() {
                        return self.err("unterminated string");
                    }
                    let e = self.s[self.i];
                    self.i += 1;
                    match e {
                        b'n' => cur.push(b'\n'),
                        b't' => cur.push(b'\t'),
                        b'r' => cur.push(b'\r'),
                        b'b' => cur.push(8),
                        b'f' => cur.push(12),
                        b'/' => cur.push(b'/'),
                        b'\\' => cur.push(b'\\'),
                        b'"' => cur.push(b'"'),
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp)
                                && self.s[self.i..].starts_with(b"\\u")
                            {
                                self.i += 2;
                                let lo = self.hex4()?;
                                if (0xDC00..0xE000).contains(&lo) {
                                    cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                } else {
                                    cp = 0xFFFD;
                                }
                            }
                            let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
                            let mut b = [0u8; 4];
                            cur.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                        }
                        b'(' => {
                            if !cur.is_empty() {
                                parts.push(SPart::Lit(String::from_utf8_lossy(&cur).into_owned()));
                                cur.clear();
                            }
                            let toks = self.tokens(true)?;
                            parts.push(SPart::Interp(toks));
                        }
                        _ => return self.err("invalid escape"),
                    }
                }
                b'\n' => {
                    self.line += 1;
                    cur.push(c);
                }
                _ => cur.push(c),
            }
        }
        if !cur.is_empty() || parts.is_empty() {
            parts.push(SPart::Lit(String::from_utf8_lossy(&cur).into_owned()));
        }
        Ok(parts)
    }

    fn hex4(&mut self) -> Result<u32, String> {
        if self.i + 4 > self.s.len() {
            return self.err("bad \\u escape");
        }
        let h = std::str::from_utf8(&self.s[self.i..self.i + 4]).map_err(|e| e.to_string())?;
        let v = u32::from_str_radix(h, 16).map_err(|_| "bad \\u escape".to_string())?;
        self.i += 4;
        Ok(v)
    }
}

pub struct Parser {
    toks: Vec<(Tok, usize)>,
    pos: usize,
}

/// Parse a program, returning its leading `include "path";` directives.
pub fn parse_program(src: &str) -> Result<(Vec<String>, Expr), String> {
    let toks = Lexer::new(src).tokens(false)?;
    let mut p = Parser { toks, pos: 0 };
    let includes = p.includes()?;
    if p.pos >= p.toks.len() {
        return Ok((includes, Expr::Identity));
    }
    let e = p.pipe(false)?;
    if p.pos < p.toks.len() {
        return p.err("unexpected token");
    }
    Ok((includes, e))
}

/// Parse a module of definitions only (the builtin prelude, included libraries).
pub fn parse_defs(src: &str) -> Result<Vec<FuncDef>, String> {
    Ok(parse_library(src)?.1)
}

pub fn parse_library(src: &str) -> Result<(Vec<String>, Vec<FuncDef>), String> {
    let toks = Lexer::new(src).tokens(false)?;
    let mut p = Parser { toks, pos: 0 };
    let includes = p.includes()?;
    let mut defs = vec![];
    while p.pos < p.toks.len() {
        if !p.is_kw("def") {
            return p.err("library should only have function definitions");
        }
        defs.push(p.funcdef()?);
    }
    Ok((includes, defs))
}

impl Parser {
    /// `include "path";` directives (optional metadata object ignored).
    fn includes(&mut self) -> Result<Vec<String>, String> {
        let mut out = vec![];
        while self.is_kw("include") {
            self.pos += 1;
            match self.next() {
                Some(Tok::Str(parts)) => match parts.as_slice() {
                    [SPart::Lit(s)] => out.push(s.clone()),
                    _ => return self.err("include path must be a constant string"),
                },
                _ => return self.err("expected a path string after include"),
            }
            if self.is_p("{") {
                self.pos += 1;
                self.object()?;
            }
            self.expect_p(";")?;
        }
        if self.is_kw("import") {
            return self.err("import is not supported (use include)");
        }
        Ok(out)
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.0)
    }
    fn peek2(&self) -> Option<&Tok> {
        self.toks.get(self.pos + 1).map(|t| &t.0)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).map(|t| t.0.clone());
        self.pos += 1;
        t
    }
    fn line(&self) -> usize {
        self.toks.get(self.pos).or(self.toks.last()).map(|t| t.1).unwrap_or(1)
    }
    fn err<T>(&self, msg: &str) -> Result<T, String> {
        let near = match self.peek() {
            Some(t) => format!("{:?}", t),
            None => "end of input".into(),
        };
        Err(format!("syntax error: {} near {} at line {}", msg, near, self.line()))
    }
    fn is_p(&self, p: &str) -> bool {
        matches!(self.peek(), Some(Tok::P(x)) if *x == p)
    }
    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Some(Tok::Kw(x)) if *x == k)
    }
    fn eat_p(&mut self, p: &str) -> bool {
        if self.is_p(p) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect_p(&mut self, p: &str) -> Result<(), String> {
        if self.eat_p(p) {
            Ok(())
        } else {
            self.err(&format!("expected '{}'", p))
        }
    }
    fn expect_kw(&mut self, k: &str) -> Result<(), String> {
        if self.is_kw(k) {
            self.pos += 1;
            Ok(())
        } else {
            self.err(&format!("expected '{}'", k))
        }
    }

    /// Pipe level (lowest precedence). `no_comma` is used for object values.
    /// Query (lowest precedence): `|`, `,`, and the right-absorbing forms
    /// `def ...; Q`, `label $x | Q`, `Expr as $p | Q` (jq 1.8 grammar).
    fn pipe(&mut self, no_comma: bool) -> Result<Expr, String> {
        let (lhs, absorbed) = self.comma_seq(no_comma)?;
        if absorbed {
            return Ok(lhs);
        }
        if self.eat_p("|") {
            let rhs = self.pipe(no_comma)?;
            return Ok(Expr::Pipe(lhs.b(), rhs.b()));
        }
        Ok(lhs)
    }

    fn query_head(&mut self, no_comma: bool) -> Result<(Expr, bool), String> {
        if self.is_kw("def") {
            let mut defs = vec![];
            while self.is_kw("def") {
                defs.push(self.funcdef()?);
            }
            let body = self.pipe(no_comma)?;
            return Ok((Expr::Defs(defs, body.b()), true));
        }
        if self.is_kw("label") {
            self.pos += 1;
            let name = match self.next() {
                Some(Tok::Var(n)) => n,
                _ => return self.err("expected $name after label"),
            };
            self.expect_p("|")?;
            let body = self.pipe(no_comma)?;
            return Ok((Expr::Label(name, body.b()), true));
        }
        let e = self.binop(0)?;
        if self.is_kw("as") {
            self.pos += 1;
            let mut pats = vec![self.pattern()?];
            while self.eat_p("?//") {
                pats.push(self.pattern()?);
            }
            self.expect_p("|")?;
            let body = self.pipe(no_comma)?;
            return Ok((Expr::Bind(e.b(), pats, body.b()), true));
        }
        Ok((e, false))
    }

    fn comma_seq(&mut self, no_comma: bool) -> Result<(Expr, bool), String> {
        let (mut acc, absorbed) = self.query_head(no_comma)?;
        if absorbed || no_comma {
            return Ok((acc, absorbed));
        }
        while self.eat_p(",") {
            let (e, absorbed) = self.query_head(false)?;
            acc = Expr::Comma(acc.b(), e.b());
            if absorbed {
                return Ok((acc, true));
            }
        }
        Ok((acc, false))
    }
    // precedence levels: 0 '//' (right), 1 assignment (nonassoc), 2 or, 3 and,
    // 4 comparison (nonassoc), 5 + -, 6 * / %
    fn binop(&mut self, min: u8) -> Result<Expr, String> {
        let mut lhs = self.unary()?;
        loop {
            let (prec, op) = match self.peek() {
                Some(Tok::P("//")) => (0, "//"),
                Some(Tok::P(p @ ("=" | "|=" | "+=" | "-=" | "*=" | "/=" | "%=" | "//="))) => (1, *p),
                Some(Tok::Kw("or")) => (2, "or"),
                Some(Tok::Kw("and")) => (3, "and"),
                Some(Tok::P(p @ ("==" | "!=" | "<" | "<=" | ">" | ">="))) => (4, *p),
                Some(Tok::P(p @ ("+" | "-"))) => (5, *p),
                Some(Tok::P(p @ ("*" | "/" | "%"))) => (6, *p),
                _ => break,
            };
            if prec < min {
                break;
            }
            self.pos += 1;
            // right-assoc for '//', nonassoc handled by using prec+1 for rhs
            let next_min = if prec == 0 { 0 } else { prec + 1 };
            let rhs = self.binop(next_min)?;
            lhs = match op {
                "//" => Expr::Alt(lhs.b(), rhs.b()),
                "=" => Expr::Assign(AssignOp::Set, lhs.b(), rhs.b()),
                "|=" => Expr::Assign(AssignOp::Update, lhs.b(), rhs.b()),
                "+=" => Expr::Assign(AssignOp::Arith(BinOp::Add), lhs.b(), rhs.b()),
                "-=" => Expr::Assign(AssignOp::Arith(BinOp::Sub), lhs.b(), rhs.b()),
                "*=" => Expr::Assign(AssignOp::Arith(BinOp::Mul), lhs.b(), rhs.b()),
                "/=" => Expr::Assign(AssignOp::Arith(BinOp::Div), lhs.b(), rhs.b()),
                "%=" => Expr::Assign(AssignOp::Arith(BinOp::Mod), lhs.b(), rhs.b()),
                "//=" => Expr::Assign(AssignOp::Alt, lhs.b(), rhs.b()),
                "or" => Expr::Or(lhs.b(), rhs.b()),
                "and" => Expr::And(lhs.b(), rhs.b()),
                "==" => Expr::Bin(BinOp::Eq, lhs.b(), rhs.b()),
                "!=" => Expr::Bin(BinOp::Ne, lhs.b(), rhs.b()),
                "<" => Expr::Bin(BinOp::Lt, lhs.b(), rhs.b()),
                "<=" => Expr::Bin(BinOp::Le, lhs.b(), rhs.b()),
                ">" => Expr::Bin(BinOp::Gt, lhs.b(), rhs.b()),
                ">=" => Expr::Bin(BinOp::Ge, lhs.b(), rhs.b()),
                "+" => Expr::Bin(BinOp::Add, lhs.b(), rhs.b()),
                "-" => Expr::Bin(BinOp::Sub, lhs.b(), rhs.b()),
                "*" => Expr::Bin(BinOp::Mul, lhs.b(), rhs.b()),
                "/" => Expr::Bin(BinOp::Div, lhs.b(), rhs.b()),
                "%" => Expr::Bin(BinOp::Mod, lhs.b(), rhs.b()),
                _ => unreachable!(),
            };
            // non-associative levels: disallow chaining
            if prec == 1 || prec == 4 {
                if let Some(Tok::P(p)) = self.peek() {
                    let chain = if prec == 1 {
                        matches!(*p, "=" | "|=" | "+=" | "-=" | "*=" | "/=" | "%=" | "//=")
                    } else {
                        matches!(*p, "==" | "!=" | "<" | "<=" | ">" | ">=")
                    };
                    if chain {
                        return self.err("non-associative operator");
                    }
                }
            }
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat_p("-") {
            let e = self.unary()?;
            if let Expr::Lit(Lit::Num(n)) = e {
                return Ok(Expr::Lit(Lit::Num(-n)));
            }
            if let Expr::Lit(Lit::NumText(t)) = &e {
                return Ok(Expr::Lit(Lit::NumText(format!("-{}", t))));
            }
            return Ok(Expr::Neg(e.b()));
        }
        self.postfix()
    }

    /// A term with postfix suffixes (`.a`, `[i]`, `[]`, `[a:b]`, `?`).
    fn postfix(&mut self) -> Result<Expr, String> {
        let mut t = self.primary()?;
        loop {
            match self.peek() {
                Some(Tok::Field(name)) => {
                    let name = name.clone();
                    self.pos += 1;
                    t = Expr::Index(t.b(), Expr::str(&name).b());
                }
                Some(Tok::P(".")) if matches!(self.peek2(), Some(Tok::Str(_))) => {
                    self.pos += 1;
                    let s = self.string_expr(None)?;
                    t = Expr::Index(t.b(), s.b());
                }
                Some(Tok::P(".")) if matches!(self.peek2(), Some(Tok::P("["))) => {
                    self.pos += 1;
                }
                Some(Tok::P("[")) => {
                    self.pos += 1;
                    t = self.bracket_suffix(t)?;
                }
                Some(Tok::P("?")) => {
                    self.pos += 1;
                    t = Expr::Try(t.b(), None);
                }
                _ => break,
            }
        }
        Ok(t)
    }

    fn bracket_suffix(&mut self, t: Expr) -> Result<Expr, String> {
        if self.eat_p("]") {
            return Ok(Expr::Iterate(t.b()));
        }
        if self.eat_p(":") {
            let to = self.pipe(false)?;
            self.expect_p("]")?;
            return Ok(Expr::Slice(t.b(), None, Some(to.b())));
        }
        let idx = self.pipe(false)?;
        if self.eat_p(":") {
            if self.eat_p("]") {
                return Ok(Expr::Slice(t.b(), Some(idx.b()), None));
            }
            let to = self.pipe(false)?;
            self.expect_p("]")?;
            return Ok(Expr::Slice(t.b(), Some(idx.b()), Some(to.b())));
        }
        self.expect_p("]")?;
        Ok(Expr::Index(t.b(), idx.b()))
    }

    fn string_expr(&mut self, fmt: Option<String>) -> Result<Expr, String> {
        let parts = match self.next() {
            Some(Tok::Str(p)) => p,
            _ => return self.err("expected string"),
        };
        let mut out = vec![];
        for p in parts {
            match p {
                SPart::Lit(s) => out.push(StrPart::Lit(s)),
                SPart::Interp(toks) => {
                    let mut sub = Parser { toks, pos: 0 };
                    let e = sub.pipe(false)?;
                    if sub.pos < sub.toks.len() {
                        return sub.err("unexpected token in interpolation");
                    }
                    out.push(StrPart::Expr(e));
                }
            }
        }
        if fmt.is_none() && out.len() == 1 {
            if let StrPart::Lit(s) = &out[0] {
                return Ok(Expr::Lit(Lit::Str(s.clone())));
            }
        }
        Ok(Expr::Interp(fmt, out))
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let line = self.line();
        let t = match self.peek() {
            Some(t) => t.clone(),
            None => return self.err("unexpected end of input"),
        };
        match t {
            Tok::Num(n) => {
                self.pos += 1;
                Ok(Expr::Lit(Lit::Num(n)))
            }
            Tok::NumText(t) => {
                self.pos += 1;
                Ok(Expr::Lit(Lit::NumText(t)))
            }
            Tok::Str(_) => self.string_expr(None),
            Tok::Format(f) => {
                self.pos += 1;
                if matches!(self.peek(), Some(Tok::Str(_))) {
                    self.string_expr(Some(f))
                } else {
                    Ok(Expr::Format(f))
                }
            }
            Tok::Field(name) => {
                self.pos += 1;
                Ok(Expr::Index(Expr::Identity.b(), Expr::str(&name).b()))
            }
            Tok::P(".") => {
                self.pos += 1;
                if matches!(self.peek(), Some(Tok::Str(_))) {
                    let s = self.string_expr(None)?;
                    return Ok(Expr::Index(Expr::Identity.b(), s.b()));
                }
                Ok(Expr::Identity)
            }
            Tok::P("..") => {
                self.pos += 1;
                Ok(Expr::Call("recurse".into(), vec![]))
            }
            Tok::Var(name) => {
                self.pos += 1;
                if name == "__loc__" {
                    return Ok(loc_obj(line));
                }
                Ok(Expr::Var(name))
            }
            Tok::Kw("__loc__") => self.err("unexpected __loc__"),
            Tok::P("(") => {
                self.pos += 1;
                let e = self.pipe(false)?;
                self.expect_p(")")?;
                Ok(e)
            }
            Tok::P("[") => {
                self.pos += 1;
                if self.eat_p("]") {
                    return Ok(Expr::Array(None));
                }
                let e = self.pipe(false)?;
                self.expect_p("]")?;
                Ok(Expr::Array(Some(e.b())))
            }
            Tok::P("{") => {
                self.pos += 1;
                self.object()
            }
            Tok::Kw("if") => {
                self.pos += 1;
                self.if_rest()
            }
            Tok::Kw("try") => {
                self.pos += 1;
                let body = self.try_operand()?;
                if self.is_kw("catch") {
                    self.pos += 1;
                    let h = self.try_operand()?;
                    return Ok(Expr::Try(body.b(), Some(h.b())));
                }
                Ok(Expr::Try(body.b(), None))
            }
            Tok::Kw("reduce") => {
                self.pos += 1;
                let src = self.binop(0)?;
                self.expect_kw("as")?;
                let pat = self.pattern()?;
                self.expect_p("(")?;
                let init = self.pipe(false)?;
                self.expect_p(";")?;
                let upd = self.pipe(false)?;
                self.expect_p(")")?;
                Ok(Expr::Reduce(src.b(), pat, init.b(), upd.b()))
            }
            Tok::Kw("foreach") => {
                self.pos += 1;
                let src = self.binop(0)?;
                self.expect_kw("as")?;
                let pat = self.pattern()?;
                self.expect_p("(")?;
                let init = self.pipe(false)?;
                self.expect_p(";")?;
                let upd = self.pipe(false)?;
                let ext = if self.eat_p(";") { Some(self.pipe(false)?.b()) } else { None };
                self.expect_p(")")?;
                Ok(Expr::Foreach(src.b(), pat, init.b(), upd.b(), ext))
            }
            Tok::Kw("label") => {
                self.pos += 1;
                let name = match self.next() {
                    Some(Tok::Var(n)) => n,
                    _ => return self.err("expected $name after label"),
                };
                self.expect_p("|")?;
                let body = self.pipe(false)?;
                Ok(Expr::Label(name, body.b()))
            }
            Tok::Kw("def") => {
                let mut defs = vec![];
                while self.is_kw("def") {
                    defs.push(self.funcdef()?);
                }
                let body = self.pipe(false)?;
                Ok(Expr::Defs(defs, body.b()))
            }
            Tok::Ident(name) => {
                self.pos += 1;
                if args_none_literal(&name, self.peek()) {
                    return Ok(match name.as_str() {
                        "true" => Expr::Lit(Lit::True),
                        "false" => Expr::Lit(Lit::False),
                        _ => Expr::Lit(Lit::Null),
                    });
                }
                if name == "break" {
                    if let Some(Tok::Var(v)) = self.peek().cloned() {
                        self.pos += 1;
                        return Ok(Expr::Break(v));
                    }
                }
                let mut args = vec![];
                if self.eat_p("(") {
                    loop {
                        args.push(self.pipe(false)?);
                        if self.eat_p(";") {
                            continue;
                        }
                        self.expect_p(")")?;
                        break;
                    }
                }
                if name == "__loc__" {
                    let _ = line;
                }
                Ok(Expr::Call(name, args))
            }
            _ => self.err("unexpected token"),
        }
    }

    /// `try`/`catch` operands bind tightly (a postfix term, optionally negated).
    fn try_operand(&mut self) -> Result<Expr, String> {
        if self.eat_p("-") {
            let e = self.try_operand()?;
            return Ok(Expr::Neg(e.b()));
        }
        self.postfix()
    }

    fn if_rest(&mut self) -> Result<Expr, String> {
        let cond = self.pipe(false)?;
        self.expect_kw("then")?;
        let then = self.pipe(false)?;
        if self.is_kw("elif") {
            self.pos += 1;
            let rest = self.if_rest()?;
            return Ok(Expr::If(cond.b(), then.b(), Some(rest.b())));
        }
        if self.is_kw("else") {
            self.pos += 1;
            let els = self.pipe(false)?;
            self.expect_kw("end")?;
            return Ok(Expr::If(cond.b(), then.b(), Some(els.b())));
        }
        self.expect_kw("end")?;
        Ok(Expr::If(cond.b(), then.b(), None))
    }

    fn object(&mut self) -> Result<Expr, String> {
        let mut entries = vec![];
        if self.eat_p("}") {
            return Ok(Expr::Object(entries));
        }
        loop {
            let line = self.line();
            let t = self.next();
            let (key, shorthand): (Expr, Option<Expr>) = match t {
                Some(Tok::Ident(n)) => (Expr::str(&n), Some(Expr::Index(Expr::Identity.b(), Expr::str(&n).b()))),
                Some(Tok::Kw(k)) if k != "__loc__" => {
                    (Expr::str(k), Some(Expr::Index(Expr::Identity.b(), Expr::str(k).b())))
                }
                Some(Tok::Var(v)) => {
                    if v != "__loc__" && self.is_p(":") {
                        // `$k: v` uses the variable's value as the key
                        (Expr::Var(v.clone()), None)
                    } else {
                        let val = if v == "__loc__" { loc_obj(line) } else { Expr::Var(v.clone()) };
                        (Expr::str(&v), Some(val))
                    }
                }
                Some(Tok::P("$")) => return self.err("bad object key"),
                Some(Tok::Str(_)) => {
                    self.pos -= 1;
                    let s = self.string_expr(None)?;
                    let sh = Expr::Index(Expr::Identity.b(), s.clone().b());
                    (s, Some(sh))
                }
                Some(Tok::Format(f)) => {
                    if matches!(self.peek(), Some(Tok::Str(_))) {
                        let s = self.string_expr(Some(f))?;
                        let sh = Expr::Index(Expr::Identity.b(), s.clone().b());
                        (s, Some(sh))
                    } else {
                        return self.err("bad object key");
                    }
                }
                Some(Tok::Num(n)) => {
                    // jq allows numeric literal keys only as errors; treat like expression
                    (Expr::Lit(Lit::Num(n)), None)
                }
                Some(Tok::P("(")) => {
                    let e = self.pipe(false)?;
                    self.expect_p(")")?;
                    (e, None)
                }
                _ => return self.err("bad object key"),
            };
            let value = if self.eat_p(":") {
                self.objval()?
            } else {
                match shorthand {
                    Some(s) => s,
                    None => return self.err("expected ':' after object key"),
                }
            };
            entries.push((key, value));
            if self.eat_p(",") {
                continue;
            }
            self.expect_p("}")?;
            break;
        }
        Ok(Expr::Object(entries))
    }

    fn objval(&mut self) -> Result<Expr, String> {
        // ExpD: ExpD '|' ExpD | '-' ExpD | Term  -- we accept any non-comma expression
        let lhs = self.binop(0)?;
        if self.eat_p("|") {
            let rhs = self.objval()?;
            return Ok(Expr::Pipe(lhs.b(), rhs.b()));
        }
        Ok(lhs)
    }

    fn pattern(&mut self) -> Result<Pattern, String> {
        match self.next() {
            Some(Tok::Var(v)) => Ok(Pattern::Var(v)),
            Some(Tok::P("[")) => {
                let mut els = vec![];
                if self.eat_p("]") {
                    return self.err("empty array pattern");
                }
                loop {
                    els.push(self.pattern()?);
                    if self.eat_p(",") {
                        continue;
                    }
                    self.expect_p("]")?;
                    break;
                }
                Ok(Pattern::Array(els))
            }
            Some(Tok::P("{")) => {
                let mut ents = vec![];
                loop {
                    match self.next() {
                        Some(Tok::Var(v)) => {
                            let sub = if self.eat_p(":") { Some(self.pattern()?) } else { None };
                            ents.push((Expr::str(&v), Some(v), sub));
                        }
                        Some(Tok::Ident(n)) => {
                            self.expect_p(":")?;
                            let sub = self.pattern()?;
                            ents.push((Expr::str(&n), None, Some(sub)));
                        }
                        Some(Tok::Kw(k)) => {
                            self.expect_p(":")?;
                            let sub = self.pattern()?;
                            ents.push((Expr::str(k), None, Some(sub)));
                        }
                        Some(Tok::Str(_)) => {
                            self.pos -= 1;
                            let s = self.string_expr(None)?;
                            self.expect_p(":")?;
                            let sub = self.pattern()?;
                            ents.push((s, None, Some(sub)));
                        }
                        Some(Tok::P("(")) => {
                            let e = self.pipe(false)?;
                            self.expect_p(")")?;
                            self.expect_p(":")?;
                            let sub = self.pattern()?;
                            ents.push((e, None, Some(sub)));
                        }
                        _ => return self.err("bad object pattern"),
                    }
                    if self.eat_p(",") {
                        continue;
                    }
                    self.expect_p("}")?;
                    break;
                }
                Ok(Pattern::Object(ents))
            }
            _ => self.err("bad pattern"),
        }
    }

    fn funcdef(&mut self) -> Result<FuncDef, String> {
        self.expect_kw("def")?;
        let name = match self.next() {
            Some(Tok::Ident(n)) => n,
            Some(Tok::Kw(k)) => k.to_string(),
            _ => return self.err("expected function name"),
        };
        let mut params = vec![];
        if self.eat_p("(") {
            loop {
                match self.next() {
                    Some(Tok::Ident(n)) => params.push(Param::Filter(n)),
                    Some(Tok::Var(n)) => params.push(Param::Value(n)),
                    _ => return self.err("bad parameter"),
                }
                if self.eat_p(";") {
                    continue;
                }
                self.expect_p(")")?;
                break;
            }
        }
        self.expect_p(":")?;
        let body = self.pipe(false)?;
        self.expect_p(";")?;
        Ok(FuncDef { name, params, body })
    }
}

fn loc_obj(line: usize) -> Expr {
    Expr::Object(vec![
        (Expr::str("file"), Expr::str("<top-level>")),
        (Expr::str("line"), Expr::Lit(Lit::Num(line as f64))),
    ])
}

fn args_none_literal(name: &str, next: Option<&Tok>) -> bool {
    matches!(name, "true" | "false" | "null") && !matches!(next, Some(Tok::P("(")))
}
