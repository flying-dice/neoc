//! Prop value forms: a prop is a **literal**, a **ref** or a **`$fn` call** from the closed,
//! pure, presentation-only function set.
//!
//! ```json
//! { "$ref": "client.search" }
//! { "$fn": "concat", "args": [ { "$ref": "src.s_notes.total" }, " notes" ] }
//! { "$lit": { "$ref": "an object that really has this key" } }
//! ```
//!
//! On the wire a prop value stays a plain [`serde_json::Value`]; [`Expr`] is the typed view of one.

use std::fmt;

use serde_json::{json, Map, Value};

/// The state namespaces a [`Ref`] can point into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Namespace {
    /// Client-managed state (the client is authoritative).
    Client,
    /// App or window state (the host is authoritative).
    State,
    /// A field of a source (`src.<id>.<field>`).
    Src,
    /// The current row inside a row template.
    Row,
    /// The current route inside a navigation destination.
    Route,
    /// The environment: size class, locale, time zone, scheme, now.
    Env,
}

impl Namespace {
    pub const ALL: [Self; 6] = [Self::Client, Self::State, Self::Src, Self::Row, Self::Route, Self::Env];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::State => "state",
            Self::Src => "src",
            Self::Row => "row",
            Self::Route => "route",
            Self::Env => "env",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|n| n.as_str() == text)
    }
}

/// The `env.*` names a ref may use.
pub const ENV_NAMES: &[&str] = &["sizeClass", "heightClass", "locale", "timeZone", "scheme", "now"];

/// A parsed `$ref` path such as `client.search` or `src.s_note.body`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Ref {
    pub namespace: Namespace,
    /// Everything after the namespace (`search`, `s_note.body`).
    pub rest: String,
}

impl Ref {
    /// Parse `namespace.rest`.
    ///
    /// # Errors
    /// [`ExprError::BadRef`] when the namespace is unknown or the rest is empty or malformed.
    pub fn parse(path: &str) -> Result<Self, ExprError> {
        let bad = || ExprError::BadRef(path.to_owned());
        let (ns, rest) = path.split_once('.').ok_or_else(bad)?;
        let namespace = Namespace::parse(ns).ok_or_else(bad)?;
        let well_formed = !rest.is_empty() && rest.split('.').all(|seg| !seg.is_empty());
        let env_ok = namespace != Namespace::Env || ENV_NAMES.contains(&rest);
        let src_ok = namespace != Namespace::Src || rest.contains('.');
        if well_formed && env_ok && src_ok {
            Ok(Self { namespace, rest: rest.to_owned() })
        } else {
            Err(bad())
        }
    }

    /// May a `Binding` prop point here? Only `client.*`, `state.*`, `src.<id>.<field>` and `row.*`
    /// are writable; `route.*` and `env.*` are read-only.
    #[must_use]
    pub const fn is_writable(&self) -> bool {
        matches!(self.namespace, Namespace::Client | Namespace::State | Namespace::Src | Namespace::Row)
    }

    /// The source id of a `src.<id>.<field>` ref.
    #[must_use]
    pub fn source_id(&self) -> Option<&str> {
        (self.namespace == Namespace::Src).then(|| self.rest.split('.').next()).flatten()
    }

    /// The first path segment of a `client.*` ref: the declared client-state name.
    #[must_use]
    pub fn client_name(&self) -> Option<&str> {
        (self.namespace == Namespace::Client).then(|| self.rest.split('.').next()).flatten()
    }
}

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.namespace.as_str(), self.rest)
    }
}

/// The closed function set. Pure, presentation-only, no loops, no recursion, no I/O.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Func {
    Not,
    And,
    Or,
    Eq,
    Ne,
    Lt,
    Gt,
    Add,
    Sub,
    If,
    Coalesce,
    Concat,
    Len,
    IsEmpty,
    Upper,
    Lower,
    FmtNumber,
    FmtCurrency,
    FmtPercent,
    FmtBytes,
    FmtDate,
    FmtTime,
    FmtDateTime,
    FmtRelative,
    FmtPlural,
    FmtList,
}

/// How many arguments a function takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arity {
    pub min: usize,
    /// `None` = variadic.
    pub max: Option<usize>,
}

impl Arity {
    const fn exactly(n: usize) -> Self {
        Self { min: n, max: Some(n) }
    }
    const fn between(min: usize, max: usize) -> Self {
        Self { min, max: Some(max) }
    }
    const fn at_least(min: usize) -> Self {
        Self { min, max: None }
    }
    #[must_use]
    pub const fn accepts(self, n: usize) -> bool {
        if n < self.min {
            return false;
        }
        match self.max {
            Some(max) => n <= max,
            None => true,
        }
    }
}

impl Func {
    /// Every function, in catalogue order.
    pub const ALL: [Self; 26] = [
        Self::Not,
        Self::And,
        Self::Or,
        Self::Eq,
        Self::Ne,
        Self::Lt,
        Self::Gt,
        Self::Add,
        Self::Sub,
        Self::If,
        Self::Coalesce,
        Self::Concat,
        Self::Len,
        Self::IsEmpty,
        Self::Upper,
        Self::Lower,
        Self::FmtNumber,
        Self::FmtCurrency,
        Self::FmtPercent,
        Self::FmtBytes,
        Self::FmtDate,
        Self::FmtTime,
        Self::FmtDateTime,
        Self::FmtRelative,
        Self::FmtPlural,
        Self::FmtList,
    ];

    /// The wire name (`"fmt.date"`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Not => "not",
            Self::And => "and",
            Self::Or => "or",
            Self::Eq => "eq",
            Self::Ne => "ne",
            Self::Lt => "lt",
            Self::Gt => "gt",
            Self::Add => "add",
            Self::Sub => "sub",
            Self::If => "if",
            Self::Coalesce => "coalesce",
            Self::Concat => "concat",
            Self::Len => "len",
            Self::IsEmpty => "isEmpty",
            Self::Upper => "upper",
            Self::Lower => "lower",
            Self::FmtNumber => "fmt.number",
            Self::FmtCurrency => "fmt.currency",
            Self::FmtPercent => "fmt.percent",
            Self::FmtBytes => "fmt.bytes",
            Self::FmtDate => "fmt.date",
            Self::FmtTime => "fmt.time",
            Self::FmtDateTime => "fmt.dateTime",
            Self::FmtRelative => "fmt.relative",
            Self::FmtPlural => "fmt.plural",
            Self::FmtList => "fmt.list",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.name() == name)
    }

    #[must_use]
    pub const fn arity(self) -> Arity {
        match self {
            Self::Not | Self::Len | Self::IsEmpty | Self::Upper | Self::Lower => Arity::exactly(1),
            Self::Eq | Self::Ne | Self::Lt | Self::Gt | Self::Add | Self::Sub | Self::FmtPlural => {
                Arity::exactly(2)
            }
            Self::If => Arity::exactly(3),
            Self::And | Self::Or | Self::Coalesce | Self::Concat => Arity::at_least(1),
            Self::FmtNumber
            | Self::FmtPercent
            | Self::FmtBytes
            | Self::FmtDate
            | Self::FmtTime
            | Self::FmtDateTime
            | Self::FmtRelative
            | Self::FmtList => Arity::between(1, 2),
            Self::FmtCurrency => Arity::between(1, 3),
        }
    }

    /// One line for the reference docs.
    #[must_use]
    pub const fn doc(self) -> &'static str {
        match self {
            Self::Not => "Boolean negation.",
            Self::And => "True when every argument is truthy.",
            Self::Or => "True when any argument is truthy.",
            Self::Eq => "Strict equality.",
            Self::Ne => "Strict inequality.",
            Self::Lt => "Less than.",
            Self::Gt => "Greater than.",
            Self::Add => "Number addition.",
            Self::Sub => "Number subtraction.",
            Self::If => "`if(cond, then, else)`.",
            Self::Coalesce => "The first argument that is not null or empty.",
            Self::Concat => "Concatenate the arguments as text.",
            Self::Len => "Length of a string or list.",
            Self::IsEmpty => "True for null, an empty string or an empty list.",
            Self::Upper => "Upper-case text.",
            Self::Lower => "Lower-case text.",
            Self::FmtNumber => "Format a number in the viewer's locale.",
            Self::FmtCurrency => "Format money in the viewer's locale.",
            Self::FmtPercent => "Format a ratio as a percentage.",
            Self::FmtBytes => "Format a byte count (KB, MB, ...).",
            Self::FmtDate => "Format a date or instant in the viewer's locale and time zone.",
            Self::FmtTime => "Format a time of day.",
            Self::FmtDateTime => "Format a date and time.",
            Self::FmtRelative => "Relative time (\"2 minutes ago\"), ticking from `env.now`.",
            Self::FmtPlural => "Pick a plural form: `fmt.plural(n, {one, other})`.",
            Self::FmtList => "Join a list (\"a, b and c\").",
        }
    }
}

/// Why a value is not a well-formed expression.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ExprError {
    #[error("`{0}` is not a valid ref (expected client.x, state.x, src.<id>.<field>, row.x, route.x or env.<name>)")]
    BadRef(String),
    #[error("`{0}` is not in the closed function set")]
    UnknownFunction(String),
    #[error("`{func}` takes {expected} argument(s), got {got}")]
    Arity { func: &'static str, expected: String, got: usize },
    #[error("malformed expression: {0}")]
    Malformed(&'static str),
}

/// The typed view of a prop value.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// A literal (any JSON value that is not a ref or a call; may contain nested refs and calls).
    Lit(Value),
    Ref(Ref),
    Call {
        func: Func,
        args: Vec<Value>,
    },
}

impl Expr {
    /// Classify a prop value.
    ///
    /// # Errors
    /// An [`ExprError`] for a bad ref, an unknown function, a wrong argument count or a malformed
    /// `{"$ref"}` / `{"$fn"}` / `{"$lit"}` object.
    pub fn parse(value: &Value) -> Result<Self, ExprError> {
        let Some(object) = value.as_object() else { return Ok(Self::Lit(value.clone())) };
        if let Some(path) = object.get("$ref") {
            let path = path.as_str().ok_or(ExprError::Malformed("`$ref` must be a string"))?;
            return Ref::parse(path).map(Self::Ref);
        }
        if let Some(name) = object.get("$fn") {
            let name = name.as_str().ok_or(ExprError::Malformed("`$fn` must be a string"))?;
            let func = Func::from_name(name).ok_or_else(|| ExprError::UnknownFunction(name.to_owned()))?;
            let args = match object.get("args") {
                None => Vec::new(),
                Some(Value::Array(args)) => args.clone(),
                Some(_) => return Err(ExprError::Malformed("`args` must be a list")),
            };
            let arity = func.arity();
            if !arity.accepts(args.len()) {
                let expected = match arity.max {
                    Some(max) if max == arity.min => max.to_string(),
                    Some(max) => format!("{}..{max}", arity.min),
                    None => format!("at least {}", arity.min),
                };
                return Err(ExprError::Arity { func: func.name(), expected, got: args.len() });
            }
            return Ok(Self::Call { func, args });
        }
        if let Some(inner) = object.get("$lit") {
            return Ok(Self::Lit(inner.clone()));
        }
        Ok(Self::Lit(value.clone()))
    }

    /// The wire form.
    #[must_use]
    pub fn to_value(&self) -> Value {
        match self {
            Self::Lit(v) => match v {
                Value::Object(o)
                    if o.contains_key("$ref") || o.contains_key("$fn") || o.contains_key("$lit") =>
                {
                    json!({ "$lit": v })
                }
                other => other.clone(),
            },
            Self::Ref(r) => json!({ "$ref": r.to_string() }),
            Self::Call { func, args } => json!({ "$fn": func.name(), "args": args }),
        }
    }
}

/// Is this value a `{"$ref"}`, `{"$fn"}` or `{"$lit"}` object?
#[must_use]
pub fn is_expression(value: &Value) -> bool {
    value.as_object().is_some_and(is_expression_object)
}

fn is_expression_object(object: &Map<String, Value>) -> bool {
    object.contains_key("$ref") || object.contains_key("$fn") || object.contains_key("$lit")
}

/// Visit every ref and call inside a value, depth first, without descending into `$lit`.
/// `at` is the key path inside the value (`""` for the value itself).
pub fn walk_exprs(value: &Value, at: &str, visit: &mut dyn FnMut(&str, Result<&Expr, &ExprError>)) {
    match value {
        Value::Object(object) if is_expression_object(object) => match Expr::parse(value) {
            Ok(expr) => {
                visit(at, Ok(&expr));
                if let Expr::Call { args, .. } = &expr {
                    for (i, arg) in args.iter().enumerate() {
                        walk_exprs(arg, &format!("{at}/args/{i}"), visit);
                    }
                }
            }
            Err(error) => visit(at, Err(&error)),
        },
        Value::Object(object) => {
            for (k, v) in object {
                walk_exprs(v, &format!("{at}/{k}"), visit);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                walk_exprs(v, &format!("{at}/{i}"), visit);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_parse_by_namespace() {
        assert_eq!(Ref::parse("client.search").unwrap().namespace, Namespace::Client);
        let r = Ref::parse("src.s_note.body").unwrap();
        assert_eq!(r.source_id(), Some("s_note"));
        assert!(r.is_writable());
        assert!(!Ref::parse("route.id").unwrap().is_writable());
        assert!(!Ref::parse("env.now").unwrap().is_writable());
        assert!(Ref::parse("row.title").unwrap().is_writable());
    }

    #[test]
    fn bad_refs_are_rejected() {
        for bad in ["nope", "weird.x", "client.", "client..x", "src.only", "env.unknown", ""] {
            assert!(Ref::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_function_set_is_the_documented_one() {
        let names: Vec<_> = Func::ALL.iter().map(|f| f.name()).collect();
        assert_eq!(names.len(), 26);
        for f in Func::ALL {
            assert_eq!(Func::from_name(f.name()), Some(f));
        }
        assert!(Func::from_name("eval").is_none());
    }

    #[test]
    fn calls_check_arity() {
        let ok = json!({ "$fn": "concat", "args": [{ "$ref": "src.s_notes.total" }, " notes"] });
        assert!(matches!(Expr::parse(&ok), Ok(Expr::Call { func: Func::Concat, .. })));
        let few = json!({ "$fn": "if", "args": [true, 1] });
        assert!(matches!(Expr::parse(&few), Err(ExprError::Arity { got: 2, .. })));
        let unknown = json!({ "$fn": "exec", "args": [] });
        assert_eq!(Expr::parse(&unknown), Err(ExprError::UnknownFunction("exec".into())));
    }

    #[test]
    fn literals_and_escapes() {
        assert_eq!(Expr::parse(&json!(3)).unwrap(), Expr::Lit(json!(3)));
        let escaped = json!({ "$lit": { "$ref": "x" } });
        let parsed = Expr::parse(&escaped).unwrap();
        assert_eq!(parsed, Expr::Lit(json!({ "$ref": "x" })));
        assert_eq!(parsed.to_value(), escaped, "a literal that looks like a ref is re-escaped");
    }

    #[test]
    fn walking_finds_nested_refs_and_stops_at_lit() {
        let value = json!({
            "text": { "$ref": "client.search" },
            "list": [{ "$fn": "len", "args": [{ "$ref": "row.items" }] }],
            "raw": { "$lit": { "$ref": "ignored.x" } }
        });
        let mut seen = Vec::new();
        walk_exprs(&value, "", &mut |at, e| {
            seen.push((at.to_owned(), matches!(e, Ok(Expr::Ref(_)))));
        });
        assert!(seen.contains(&("/text".to_owned(), true)));
        assert!(seen.contains(&("/list/0/args/0".to_owned(), true)));
        assert!(!seen.iter().any(|(at, is_ref)| at.starts_with("/raw") && *is_ref));
    }

    #[test]
    fn arity_arithmetic() {
        assert!(Arity::at_least(1).accepts(5));
        assert!(!Arity::at_least(1).accepts(0));
        assert!(Arity::between(1, 2).accepts(2));
        assert!(!Arity::between(1, 2).accepts(3));
        assert!(!Arity::exactly(2).accepts(1));
    }
}
