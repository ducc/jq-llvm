//! jq abstract syntax tree.

#[derive(Debug, Clone, PartialEq)]
pub enum Lit {
    Null,
    True,
    False,
    Num(f64),
    /// number written with text jq would preserve (e.g. `1.0`, `1e3`, huge integers)
    NumText(String),
    Str(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Set,          // =
    Update,       // |=
    Arith(BinOp), // += -= *= /= %=
    Alt,          // //=
}

#[derive(Debug, Clone)]
pub enum StrPart {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Var(String),
    Array(Vec<Pattern>),
    /// (key expression, optional variable bound to the whole value, sub-pattern)
    Object(Vec<(Expr, Option<String>, Option<Pattern>)>),
}

#[derive(Debug, Clone)]
pub enum Param {
    Filter(String),
    Value(String),
}

#[derive(Debug, Clone)]
pub struct FuncDef {
    pub name: String,
    pub params: Vec<Param>,
    pub body: Expr,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Identity,
    Lit(Lit),
    /// `@fmt` used as a filter
    Format(String),
    /// string interpolation with optional format
    Interp(Option<String>, Vec<StrPart>),
    Index(Box<Expr>, Box<Expr>),
    Slice(Box<Expr>, Option<Box<Expr>>, Option<Box<Expr>>),
    Iterate(Box<Expr>),
    Try(Box<Expr>, Option<Box<Expr>>),
    Pipe(Box<Expr>, Box<Expr>),
    Comma(Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Alt(Box<Expr>, Box<Expr>),
    Assign(AssignOp, Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Option<Box<Expr>>),
    Array(Option<Box<Expr>>),
    Object(Vec<(Expr, Expr)>),
    Var(String),
    /// source as pattern(s) | body ; several patterns = `?//` alternatives
    Bind(Box<Expr>, Vec<Pattern>, Box<Expr>),
    Reduce(Box<Expr>, Pattern, Box<Expr>, Box<Expr>),
    Foreach(Box<Expr>, Pattern, Box<Expr>, Box<Expr>, Option<Box<Expr>>),
    Label(String, Box<Expr>),
    Break(String),
    Defs(Vec<FuncDef>, Box<Expr>),
    Call(String, Vec<Expr>),
}

impl Expr {
    pub fn b(self) -> Box<Expr> {
        Box::new(self)
    }
    pub fn str(s: &str) -> Expr {
        Expr::Lit(Lit::Str(s.to_string()))
    }
}
