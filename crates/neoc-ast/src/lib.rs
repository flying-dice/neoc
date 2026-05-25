//! Typed AST for neoc. Pure data: no host or runtime types, no I/O. Nodes the
//! type checker reports against ([`Expr`], [`Stmt`], [`Item`]) carry a byte
//! [`Span`] so diagnostics can point at the offending source. Mirrors
//! `lang/SYNTAX.md`.
//!
//! Every node derives `Debug, Clone, PartialEq` so the parser can be tested by
//! asserting the produced AST against an expected value, independently of the
//! emitter. [`Span`] compares always-equal (see its `PartialEq`), so adding a
//! span to a node leaves AST-equality tests untouched.

/// An identifier — a name in either the value or type namespace.
pub type Ident = String;

/// A half-open byte range `[start, end)` into the source text the node came
/// from.
///
/// All `Span`s compare equal regardless of their offsets. This is deliberate:
/// the AST derives `PartialEq` so the parser can be tested by asserting the
/// produced tree against an expected one, and a span carries position, not
/// structure. Comparing spans structurally would force every expected-AST test
/// to thread exact byte offsets; comparing them as always-equal lets a node
/// gain a span without disturbing those tests.
#[derive(Clone, Copy, Debug, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl PartialEq for Span {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Span {}

/// A complete source file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Module {
    pub items: Vec<Item>,
}

// ---- items -----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub attrs: Vec<Attribute>,
    pub vis: Visibility,
    pub kind: ItemKind,
    pub span: Span,
}

impl Item {
    /// Returns the module named by a `#[link(...)]` attribute on this item, if
    /// present.
    ///
    /// Accepts both the positional form `#[link("module")]` and the
    /// Rust-faithful key-value form `#[link(name = "module")]`; the latter
    /// takes precedence when both are present. Used by the backends to lower an
    /// `extern "<host>" { ... }` block into a native import from the named
    /// module. Returns `None` when no `link` attribute is present or its
    /// argument is not a string.
    #[must_use]
    pub fn link_name(&self) -> Option<&str> {
        let link = self
            .attrs
            .iter()
            .flat_map(|attr| &attr.items)
            .find(|item| item.name == "link")?;
        // Prefer `name = "..."`, then fall back to the first positional string.
        link.args
            .iter()
            .find_map(|arg| match arg {
                AttrArg::Named { name, value } if name == "name" => match value.as_ref() {
                    AttrArg::Str(s) => Some(s.as_str()),
                    _ => None,
                },
                _ => None,
            })
            .or_else(|| {
                link.args.iter().find_map(|arg| match arg {
                    AttrArg::Str(s) => Some(s.as_str()),
                    _ => None,
                })
            })
    }
}

/// A backend target. A hosted `extern "<host>"` block names the runtime its
/// externals come from; that host maps to exactly one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Lua,
    Js,
}

impl Target {
    /// Maps an `extern "<host>"` host string to the backend it targets.
    ///
    /// Returns `None` for an unrecognised host. The recognised hosts are
    /// `neoc`, `lua`, `luau` (Lua); and `node`, `bun`, `deno` (JavaScript).
    #[must_use]
    pub fn from_host(host: &str) -> Option<Self> {
        match host {
            "neoc" | "lua" | "luau" => Some(Target::Lua),
            "node" | "bun" | "deno" => Some(Target::Js),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Public,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    Use(UseDecl),
    Const(ConstDecl),
    TypeAlias(TypeAlias),
    Struct(StructDecl),
    Enum(EnumDecl),
    Trait(TraitDecl),
    Impl(ImplBlock),
    Fn(FnDecl),
    /// An un-hosted ambient extern block (`ext { ... }` / `ext fn ...`). Its
    /// items are host globals provided by the runtime; no import is emitted.
    Ext(Vec<ExtItem>),
    /// A hosted external block (`extern "<host>" { ... }`). `host` names the
    /// runtime the externals come from and implies the target backend (see
    /// [`Target::from_host`]). When the item carries `#[link(name = "...")]`
    /// the matching backend emits a native import; without it the items are
    /// host globals.
    Extern {
        host: String,
        items: Vec<ExtItem>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct UseDecl {
    pub path: Vec<Ident>,
    pub alias: Option<Ident>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstDecl {
    pub name: Ident,
    pub ty: Type,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeAlias {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub value: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub body: StructBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StructBody {
    Unit,
    Tuple(Type),
    Named(Vec<Field>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub attrs: Vec<Attribute>,
    pub name: Ident,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub variants: Vec<Variant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub attrs: Vec<Attribute>,
    pub name: Ident,
    pub body: VariantBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VariantBody {
    Unit,
    Tuple(Vec<Type>),
    Named(Vec<Field>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitDecl {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub items: Vec<TraitItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitItem {
    pub sig: FnSig,
    /// Present when the trait method has a default body.
    pub default: Option<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImplBlock {
    pub generics: Vec<GenericParam>,
    /// Present for a trait impl (`impl Trait for Type`), absent for inherent.
    pub trait_: Option<TypePath>,
    pub ty: Type,
    pub items: Vec<FnDecl>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnDecl {
    pub attrs: Vec<Attribute>,
    pub sig: FnSig,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnSig {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    pub where_clause: Vec<WherePredicate>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    /// The `self` receiver — always by value (no references in neoc).
    Receiver,
    Named {
        name: Ident,
        ty: Type,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExtItem {
    Fn(FnSig),
    Type {
        name: Ident,
        generics: Vec<GenericParam>,
    },
    Const {
        name: Ident,
        ty: Type,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenericParam {
    pub name: Ident,
    pub bounds: Vec<TypePath>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WherePredicate {
    pub ty: Ident,
    pub bounds: Vec<TypePath>,
}

/// Collects the native imports a `target` backend must emit for the hosted
/// `extern "<host>" { ... }` blocks in `module`.
///
/// Only blocks whose host maps to `target` (via [`Target::from_host`]) and that
/// carry a `#[link(name = "...")]` / `#[link("...")]` module specifier
/// contribute. Returns one entry per distinct module specifier, each pairing
/// the specifier with the value names (`fn` and `const` items) declared in the
/// matching blocks. The entries are sorted by specifier and the names within
/// each entry preserve declaration order, so backends emit a deterministic
/// import preamble. Un-hosted `ext` blocks, hosted blocks without `#[link]`,
/// blocks targeting another backend, and `type` items all contribute nothing —
/// they are either host globals or erased.
#[must_use]
pub fn module_imports(module: &Module, target: Target) -> Vec<(String, Vec<Ident>)> {
    use std::collections::BTreeMap;

    let mut by_spec: BTreeMap<String, Vec<Ident>> = BTreeMap::new();
    for item in &module.items {
        let ItemKind::Extern { host, items } = &item.kind else {
            continue;
        };
        if Target::from_host(host) != Some(target) {
            continue;
        }
        let Some(spec) = item.link_name() else {
            continue;
        };
        let names = by_spec.entry(spec.to_string()).or_default();
        for ext in items {
            match ext {
                ExtItem::Fn(sig) => names.push(sig.name.clone()),
                ExtItem::Const { name, .. } => names.push(name.clone()),
                ExtItem::Type { .. } => {}
            }
        }
    }
    by_spec.into_iter().collect()
}

// ---- types -----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Unit,
    List(Box<Type>),
    Fn {
        params: Vec<Type>,
        ret: Option<Box<Type>>,
    },
    SelfType,
    Path(TypePath),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypePath {
    pub segments: Vec<TypeSegment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeSegment {
    pub name: Ident,
    pub args: Vec<Type>,
}

// ---- expressions -----------------------------------------------------------

/// An expression node: its [`ExprKind`] plus the byte [`Span`] it came from.
///
/// The checker reports diagnostics against the span of the expression it found
/// the problem in. Per [`Span`]'s `PartialEq`, two expressions with the same
/// kind compare equal regardless of span, so AST-equality tests are unaffected.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

impl Expr {
    /// Builds an expression node from its kind and the byte span it occupies in
    /// the source.
    #[must_use]
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Int(String),
    Float(String),
    Str(String),
    Bool(bool),
    Unit,
    /// A `::`-joined path: `x`, `Color::Red`, `module::f`.
    Path(Vec<Ident>),
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Unary {
        op: UnOp,
        operand: Box<Expr>,
    },
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
    },
    Try(Box<Expr>),
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    MethodCall {
        receiver: Box<Expr>,
        method: Ident,
        args: Vec<Expr>,
    },
    Field {
        base: Box<Expr>,
        field: FieldAccess,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    StructLit {
        path: Vec<Ident>,
        fields: Vec<FieldInit>,
        base: Option<Box<Expr>>,
    },
    List(Vec<Expr>),
    Assign {
        target: Box<Expr>,
        op: AssignOp,
        value: Box<Expr>,
    },
    If {
        cond: Box<Expr>,
        then: Block,
        /// A `Block` or another `If` (else-if chain).
        else_: Option<Box<Expr>>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    While {
        cond: Box<Expr>,
        body: Block,
    },
    For {
        pattern: Pattern,
        iter: Box<Expr>,
        body: Block,
    },
    Loop {
        body: Block,
    },
    Closure {
        params: Vec<ClosureParam>,
        body: Box<Expr>,
    },
    Block(Block),
    Return(Option<Box<Expr>>),
    Break,
    Continue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldAccess {
    Named(Ident),
    /// Newtype / positional field: `value.0`.
    Tuple(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldInit {
    pub name: Ident,
    /// `None` for field shorthand (`Point { x, y }`).
    pub value: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClosureParam {
    pub name: Ident,
    pub ty: Option<Type>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    /// String concatenation. Not produced by the parser — `+` always parses as
    /// `Add`. The type checker's elaboration rewrites a string-typed `Add` to
    /// `Concat` so the emitter can pick Lua `..` over `+` without re-deriving
    /// types. (`lang/DYNAMIC.md` §4.2.)
    Concat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

// ---- statements ------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// The trailing expression that is the block's value, if any.
    pub value: Option<Box<Expr>>,
}

/// A statement node: its [`StmtKind`] plus the byte [`Span`] it came from.
///
/// As with [`Expr`], the span lets the checker locate a diagnostic precisely
/// while leaving AST-equality comparisons span-insensitive.
#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

impl Stmt {
    /// Builds a statement node from its kind and the byte span it occupies in
    /// the source.
    #[must_use]
    pub fn new(kind: StmtKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Let(LetStmt),
    Item(Item),
    /// An expression evaluated for effect, terminated by `;`.
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LetStmt {
    pub mutable: bool,
    pub pattern: Pattern,
    pub ty: Option<Type>,
    pub value: Option<Expr>,
}

// ---- patterns --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard,
    Literal(LiteralPat),
    /// A lone identifier. The adapter leaves it as a binding; name resolution
    /// reinterprets it as a unit struct/variant pattern when it resolves to
    /// one (as Rust does).
    Binding(Ident),
    /// A multi-segment path: a unit variant like `Color::Red`.
    Path(Vec<Ident>),
    Struct {
        path: Vec<Ident>,
        fields: Vec<FieldPat>,
        /// A trailing `..` ignoring the remaining fields.
        rest: bool,
    },
    Tuple {
        path: Vec<Ident>,
        elems: Vec<Pattern>,
    },
    Or(Vec<Pattern>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum LiteralPat {
    Int(String),
    Float(String),
    Str(String),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldPat {
    pub name: Ident,
    /// `None` for shorthand (`Point { x, y }`).
    pub pattern: Option<Pattern>,
}

// ---- attributes ------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub items: Vec<AttrItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttrItem {
    pub name: Ident,
    pub args: Vec<AttrArg>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AttrArg {
    Item(AttrItem),
    /// A Rust-faithful key-value argument: `name = value`, e.g.
    /// `#[link(name = "color")]`.
    Named {
        name: Ident,
        value: Box<AttrArg>,
    },
    Int(String),
    Float(String),
    Str(String),
    Bool(bool),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_compare_equal_regardless_of_offset() {
        // Two spans with different offsets are `==`. AST nodes that carry a span
        // therefore compare structurally-equal regardless of position, which is
        // what keeps the parser's AST-equality tests stable.
        let a = Span { start: 0, end: 3 };
        let b = Span { start: 17, end: 42 };
        assert_eq!(a, b);

        // The same node with different spans compares equal too.
        let lhs = Expr::new(ExprKind::Int("1".into()), a);
        let rhs = Expr::new(ExprKind::Int("1".into()), b);
        assert_eq!(lhs, rhs);
    }
}
