//! `validate(node)`: checks a tree against the component schema (spec §2.1, §3.2, §5.3, §5.4).
//!
//! The host runs it on **every tree an app renders**, so a bad app fails in its own tests, not in the client.
//! It is a pure function over the catalogue tables in [`super::components`]: unknown components, props,
//! slots and events; wrong prop types; missing required props and slots; bindings that are not writable refs;
//! accessibility rules (a Button needs a name, an Image needs `alt`); keys (unique among siblings, explicit on
//! interactive nodes); test-id uniqueness; and the size limits.
//!
//! Every [`Violation`] carries the node's path and, where it applies, the field inside the node
//! (`props.text`, `on.tap`, `slots.row`), so the message points at exactly what to fix.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use serde_json::Value;

use super::components::{self, FRAME, INSETS};
use super::expr::{walk_exprs, Expr, Namespace, Ref};
use super::node::{derive_test_id, is_auto_key, join_path, limits, Action, Handler, Node};
use super::source::ViewDecls;
use super::spec::{Children, ComponentSpec, NameRule, ObjectDef, PropSpec, PropType, SlotScope};
use super::version::{kit_supports, KitVersion};

/// How bad a finding is. Errors make a tree invalid; warnings do not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

/// What kind of finding, for programmatic handling and tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Code {
    UnknownComponent,
    UnknownProp,
    WrongType,
    MissingProp,
    BindingNotRef,
    BindingNotWritable,
    UnknownSlot,
    SlotTooLarge,
    ChildrenNotAllowed,
    MissingSlot,
    UnknownEvent,
    BadHandler,
    BadExpression,
    UndeclaredRef,
    RefOutOfScope,
    MissingName,
    InvalidKey,
    InteractiveNeedsKey,
    DuplicateKey,
    DuplicateTestId,
    TooDeep,
    TooManyNodes,
    DragWithoutAlternative,
    NewerThanClient,
}

/// One finding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// The path of the node (`main/list/@row/item`).
    pub path: String,
    /// The field inside the node: `props.text`, `on.tap`, `slots.row`.
    pub field: Option<String>,
    pub severity: Severity,
    pub code: Code,
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        match &self.field {
            Some(field) => write!(f, "{level}: {} {field}: {}", self.path, self.message),
            None => write!(f, "{level}: {}: {}", self.path, self.message),
        }
    }
}

/// What the tree may refer to beyond the schema: the view's declarations and the client's kit.
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// Declared source ids. `None` = do not check `Source` props and `src.*` refs.
    pub sources: Option<BTreeSet<String>>,
    /// Declared client-state names. `None` = do not check `client.*` refs.
    pub client: Option<BTreeSet<String>>,
    /// The kit the client runs: components newer than it must carry a `fallback` slot.
    pub client_kit: Option<KitVersion>,
}

impl Context {
    /// Check refs and source props against a view's declarations.
    #[must_use]
    pub fn from_decls(decls: &ViewDecls) -> Self {
        Self {
            sources: Some(decls.sources.keys().cloned().collect()),
            client: Some(decls.client.keys().cloned().collect()),
            client_kit: None,
        }
    }
}

/// Validate a tree against the schema with no view declarations. Returns every finding; an empty list (or
/// warnings only, see [`is_valid`]) means the tree is good.
#[must_use]
pub fn validate(root: &Node) -> Vec<Violation> {
    validate_with(root, &Context::default())
}

/// Validate a tree, also checking refs and source props against `context`.
#[must_use]
pub fn validate_with(root: &Node, context: &Context) -> Vec<Violation> {
    let mut run = Run::new(context);
    let path = root.key.clone();
    run.node(root, &path, &Scope::default(), 1);
    run.out
}

/// No errors (warnings are allowed).
#[must_use]
pub fn is_valid(violations: &[Violation]) -> bool {
    violations.iter().all(|v| v.severity != Severity::Error)
}

#[derive(Clone, Default)]
struct Scope {
    in_row: bool,
    in_route: bool,
    branches: Vec<Branch>,
}

struct Run<'a> {
    context: &'a Context,
    out: Vec<Violation>,
    nodes: usize,
    too_many_reported: bool,
    too_deep_reported: bool,
    /// test id -> (path, the exclusive branches the node sits in)
    test_ids: HashMap<String, Vec<(String, Vec<Branch>)>>,
}

/// One arm of a `Show` or `Adaptive`: only one arm is mounted at a time, so arms may reuse test ids.
type Branch = (String, String);

impl<'a> Run<'a> {
    fn new(context: &'a Context) -> Self {
        Self {
            context,
            out: Vec::new(),
            nodes: 0,
            too_many_reported: false,
            too_deep_reported: false,
            test_ids: HashMap::new(),
        }
    }

    fn report(
        &mut self,
        path: &str,
        field: Option<&str>,
        severity: Severity,
        code: Code,
        message: impl Into<String>,
    ) {
        self.out.push(Violation {
            path: path.to_owned(),
            field: field.map(str::to_owned),
            severity,
            code,
            message: message.into(),
        });
    }

    fn error(&mut self, path: &str, field: Option<&str>, code: Code, message: impl Into<String>) {
        self.report(path, field, Severity::Error, code, message);
    }

    fn node(&mut self, node: &Node, path: &str, scope: &Scope, depth: usize) {
        self.nodes += 1;
        if self.nodes > limits::MAX_NODES && !self.too_many_reported {
            self.too_many_reported = true;
            self.error(
                path,
                None,
                Code::TooManyNodes,
                format!("a view holds at most {} nodes", limits::MAX_NODES),
            );
        }
        if depth > limits::MAX_DEPTH {
            if !self.too_deep_reported {
                self.too_deep_reported = true;
                self.error(
                    path,
                    None,
                    Code::TooDeep,
                    format!("trees are at most {} levels deep", limits::MAX_DEPTH),
                );
            }
            return;
        }
        self.check_key(node, path);
        let spec = components::lookup(&node.type_);
        match spec {
            None => {
                self.error(path, None, Code::UnknownComponent, format!("unknown component `{}`", node.type_));
            }
            Some(spec) => self.component(node, spec, path, scope),
        }
        self.test_id(node, path, scope);
        self.children(node, spec, path, scope, depth);
    }

    fn check_key(&mut self, node: &Node, path: &str) {
        let key = node.key.as_str();
        if key.is_empty() || key.contains('/') || key.starts_with('@') || key.chars().any(char::is_whitespace)
        {
            self.error(
                path,
                None,
                Code::InvalidKey,
                format!("key `{key}` must be non-empty, without `/`, whitespace or a leading `@`"),
            );
        }
    }

    fn component(&mut self, node: &Node, spec: &ComponentSpec, path: &str, scope: &Scope) {
        if let Some(client) = self.context.client_kit {
            if !kit_supports(client, spec.since) && !node.slots.contains_key("fallback") {
                self.report(
                    path,
                    None,
                    Severity::Warning,
                    Code::NewerThanClient,
                    format!(
                        "`{}` needs kit {} but the client runs {client}; add a `fallback` slot",
                        spec.name, spec.since
                    ),
                );
            }
        }
        self.props(node, spec, path, scope);
        self.events(node, spec, path, scope);
        self.slots(node, spec, path);
        self.interactivity(node, spec, path);
        self.accessibility(node, spec, path);
        if node.props.contains_key("drag") && !node.slots.contains_key("contextMenu") {
            self.report(
                path,
                Some("props.drag"),
                Severity::Warning,
                Code::DragWithoutAlternative,
                "a draggable node needs a non-drag path: add a `contextMenu` slot (\"Move to...\")",
            );
        }
    }

    // ---- props ----------------------------------------------------------------------------------------

    fn props(&mut self, node: &Node, spec: &ComponentSpec, path: &str, scope: &Scope) {
        for (name, value) in &node.props {
            let field = format!("props.{name}");
            let Some(prop) = spec.prop(name) else {
                if !spec.open_props {
                    self.error(
                        path,
                        Some(&field),
                        Code::UnknownProp,
                        format!("`{}` has no prop `{name}`", spec.name),
                    );
                }
                continue;
            };
            self.prop_value(prop, value, path, &field, scope);
        }
        for prop in spec.effective_props().filter(|p| p.required) {
            if !node.props.contains_key(prop.name) {
                self.error(
                    path,
                    Some(&format!("props.{}", prop.name)),
                    Code::MissingProp,
                    format!("`{}` needs `{}`", spec.name, prop.name),
                );
            }
        }
    }

    /// Check one value against its spec: expressions first, then the literal type, then bindings.
    fn prop_value(&mut self, prop: &PropSpec, value: &Value, path: &str, field: &str, scope: &Scope) {
        self.expressions(value, path, field, scope);
        if super::expr::is_expression(value) && Expr::parse(value).is_err() {
            return; // already reported
        }
        if prop.binding.is_some() {
            self.binding(value, path, field);
            return;
        }
        if matches!(Expr::parse(value), Ok(Expr::Ref(_) | Expr::Call { .. }))
            && !matches!(prop.ty, PropType::Source)
        {
            return; // evaluated on the client: its type cannot be known here
        }
        if matches!(prop.ty, PropType::Source) {
            self.source_prop(value, path, field);
            return;
        }
        for (at, message) in self.literal(&prop.ty, value, field, scope, path) {
            self.error(path, Some(&at), Code::WrongType, message);
        }
    }

    fn source_prop(&mut self, value: &Value, path: &str, field: &str) {
        let Some(id) = value.as_str() else {
            self.error(path, Some(field), Code::WrongType, "expected a source id (a string)");
            return;
        };
        if let Some(sources) = &self.context.sources {
            if !sources.contains(id) {
                self.error(
                    path,
                    Some(field),
                    Code::UndeclaredRef,
                    format!("source `{id}` is not declared for this view"),
                );
            }
        }
    }

    fn binding(&mut self, value: &Value, path: &str, field: &str) {
        match Expr::parse(value) {
            Ok(Expr::Ref(r)) if r.is_writable() => {}
            Ok(Expr::Ref(r)) => self.error(
                path,
                Some(field),
                Code::BindingNotWritable,
                format!("`{r}` is read-only; a binding needs client.*, state.*, src.<id>.<field> or row.*"),
            ),
            Ok(_) => self.error(
                path,
                Some(field),
                Code::BindingNotRef,
                "a binding takes a ref (`{\"$ref\": \"client.x\"}`), not a value",
            ),
            Err(_) => {} // reported by `expressions`
        }
    }

    /// Validate every ref and call inside a value.
    fn expressions(&mut self, value: &Value, path: &str, field: &str, scope: &Scope) {
        let mut findings: Vec<(String, Code, String)> = Vec::new();
        walk_exprs(value, "", &mut |at, expr| match expr {
            Err(error) => findings.push((at.to_owned(), Code::BadExpression, error.to_string())),
            Ok(Expr::Ref(r)) => {
                if let Some((code, message)) = self.check_ref(r, scope) {
                    findings.push((at.to_owned(), code, message));
                }
            }
            Ok(_) => {}
        });
        for (at, code, message) in findings {
            self.error(path, Some(&format!("{field}{at}")), code, message);
        }
    }

    fn check_ref(&self, r: &Ref, scope: &Scope) -> Option<(Code, String)> {
        match r.namespace {
            Namespace::Row if !scope.in_row => {
                Some((Code::RefOutOfScope, format!("`{r}`: `row.*` only exists inside a row template")))
            }
            Namespace::Route if !scope.in_route => Some((
                Code::RefOutOfScope,
                format!("`{r}`: `route.*` only exists inside a navigation destination"),
            )),
            Namespace::Client => {
                let declared = self.context.client.as_ref()?;
                let name = r.client_name()?;
                (!declared.contains(name)).then(|| {
                    (
                        Code::UndeclaredRef,
                        format!("`{r}`: client state `{name}` is not declared for this view"),
                    )
                })
            }
            Namespace::Src => {
                let declared = self.context.sources.as_ref()?;
                let id = r.source_id()?;
                (!declared.contains(id)).then(|| {
                    (Code::UndeclaredRef, format!("`{r}`: source `{id}` is not declared for this view"))
                })
            }
            _ => None,
        }
    }

    // ---- literal types --------------------------------------------------------------------------------

    /// Check a literal against a type. Returns `(field path, message)` for each problem.
    fn literal(
        &self,
        ty: &PropType,
        value: &Value,
        at: &str,
        scope: &Scope,
        path: &str,
    ) -> Vec<(String, String)> {
        let value = unescape(value);
        // refs and calls nested inside objects and lists are evaluated on the client
        if matches!(Expr::parse(value), Ok(Expr::Ref(_) | Expr::Call { .. })) {
            return Vec::new();
        }
        let wrong =
            |expected: &str| vec![(at.to_owned(), format!("expected {expected}, got {}", describe(value)))];
        match ty {
            PropType::Str | PropType::Source => {
                value.is_string().then(Vec::new).unwrap_or_else(|| wrong("a string"))
            }
            PropType::Number => value.is_number().then(Vec::new).unwrap_or_else(|| wrong("a number")),
            PropType::Int => {
                (value.is_i64() || value.is_u64()).then(Vec::new).unwrap_or_else(|| wrong("an integer"))
            }
            PropType::Bool => value.is_boolean().then(Vec::new).unwrap_or_else(|| wrong("a boolean")),
            PropType::Json => Vec::new(),
            PropType::Spacing => {
                if value.as_u64().is_some_and(|n| n <= 8) {
                    Vec::new()
                } else {
                    wrong("a spacing from 0 to 8")
                }
            }
            PropType::Enum(def) => match value.as_str() {
                Some(text) if def.values.contains(&text) => Vec::new(),
                Some(text) => vec![(at.to_owned(), format!("`{text}` is not one of {}", quoted(def.values)))],
                None => wrong(&format!("one of {}", quoted(def.values))),
            },
            PropType::Insets => {
                if value.is_u64() {
                    self.literal(&PropType::Spacing, value, at, scope, path)
                } else {
                    self.object(&INSETS, value, at, scope, path)
                }
            }
            PropType::Frame => {
                if value.as_str() == Some("fill") {
                    Vec::new()
                } else {
                    self.object(&FRAME, value, at, scope, path)
                }
            }
            PropType::List(inner) => match value.as_array() {
                Some(items) => items
                    .iter()
                    .enumerate()
                    .flat_map(|(i, item)| self.literal(inner, item, &format!("{at}/{i}"), scope, path))
                    .collect(),
                None => wrong("a list"),
            },
            PropType::Object(def) => self.object(def, value, at, scope, path),
            PropType::Union(options) => {
                if options.iter().any(|option| self.literal(option, value, at, scope, path).is_empty()) {
                    Vec::new()
                } else if let Some(fits) = options.iter().find(|option| kind_fits(option, value)) {
                    self.literal(fits, value, at, scope, path)
                } else {
                    wrong(&options.iter().map(type_name).collect::<Vec<_>>().join(" or "))
                }
            }
        }
    }

    fn object(
        &self,
        def: &ObjectDef,
        value: &Value,
        at: &str,
        scope: &Scope,
        path: &str,
    ) -> Vec<(String, String)> {
        let Some(object) = value.as_object() else {
            return vec![(at.to_owned(), format!("expected an object, got {}", describe(value)))];
        };
        let mut problems = Vec::new();
        for (name, field_value) in object {
            let field_at = format!("{at}/{name}");
            match def.fields.iter().find(|f| f.name == name) {
                Some(field) if field.binding.is_some() => {
                    if !matches!(Expr::parse(field_value), Ok(Expr::Ref(r)) if r.is_writable()) {
                        problems.push((field_at, "a binding takes a writable ref".to_owned()));
                    }
                }
                Some(field) => problems.extend(self.literal(&field.ty, field_value, &field_at, scope, path)),
                None if def.open => {}
                None => problems.push((field_at, format!("unknown field `{name}`"))),
            }
        }
        for field in def.fields.iter().filter(|f| f.required) {
            if !object.contains_key(field.name) {
                problems.push((format!("{at}/{}", field.name), format!("missing `{}`", field.name)));
            }
        }
        problems
    }

    // ---- events ---------------------------------------------------------------------------------------

    fn events(&mut self, node: &Node, spec: &ComponentSpec, path: &str, scope: &Scope) {
        for (event, handler) in &node.on {
            let field = format!("on.{event}");
            if spec.event(event).is_none() {
                self.error(
                    path,
                    Some(&field),
                    Code::UnknownEvent,
                    format!("`{}` does not report `{event}`", spec.name),
                );
                continue;
            }
            if let Handler::Actions(actions) = handler {
                for (i, action) in actions.iter().enumerate() {
                    self.action(action, path, &format!("{field}/{i}"), scope);
                }
            }
        }
    }

    fn action(&mut self, action: &Action, path: &str, field: &str, scope: &Scope) {
        match action {
            Action::Set { target, value } => {
                self.action_target(target, path, field);
                self.expressions(value, path, field, scope);
            }
            Action::Toggle { target } => self.action_target(target, path, field),
            Action::NavPush { route, .. } => {
                if !route.get("name").is_some_and(Value::is_string) {
                    self.error(
                        path,
                        Some(field),
                        Code::BadHandler,
                        "`nav.push` needs a route with a string `name`",
                    );
                }
                for (name, value) in route {
                    self.expressions(value, path, &format!("{field}/route/{name}"), scope);
                }
            }
            Action::Copy { text } => self.expressions(text, path, field, scope),
            Action::Emit { payload, .. } => {
                for (name, value) in payload.iter().flatten() {
                    self.expressions(value, path, &format!("{field}/payload/{name}"), scope);
                }
            }
            Action::Seq { actions } => {
                for (i, nested) in actions.iter().enumerate() {
                    self.action(nested, path, &format!("{field}/{i}"), scope);
                }
            }
            Action::NavPop | Action::NavPopToRoot | Action::Focus { .. } | Action::ScrollTo { .. } => {}
        }
    }

    fn action_target(&mut self, target: &str, path: &str, field: &str) {
        match Ref::parse(target) {
            Ok(r) if r.is_writable() => {
                if let Some((code, message)) =
                    self.check_ref(&r, &Scope { in_row: true, in_route: true, branches: Vec::new() })
                {
                    self.error(path, Some(field), code, message);
                }
            }
            Ok(r) => self.error(
                path,
                Some(field),
                Code::BadHandler,
                format!("an action cannot write the read-only `{r}`"),
            ),
            Err(error) => self.error(path, Some(field), Code::BadHandler, error.to_string()),
        }
    }

    // ---- slots and children ---------------------------------------------------------------------------

    fn slots(&mut self, node: &Node, spec: &ComponentSpec, path: &str) {
        for (name, nodes) in &node.slots {
            let field = format!("slots.{name}");
            match spec.slot(name) {
                None if spec.open_props => {}
                None => self.error(
                    path,
                    Some(&field),
                    Code::UnknownSlot,
                    format!("`{}` has no slot `{name}`", spec.name),
                ),
                Some(slot) => {
                    if slot.max.is_some_and(|max| nodes.len() > usize::from(max)) {
                        self.error(
                            path,
                            Some(&field),
                            Code::SlotTooLarge,
                            format!(
                                "slot `{name}` takes at most {} node(s), got {}",
                                slot.max.unwrap_or(0),
                                nodes.len()
                            ),
                        );
                    }
                }
            }
        }
        for slot in spec.effective_slots().filter(|s| s.required) {
            if node.slots.get(slot.name).is_none_or(Vec::is_empty) {
                self.error(
                    path,
                    Some(&format!("slots.{}", slot.name)),
                    Code::MissingSlot,
                    format!("`{}` needs its `{}` slot", spec.name, slot.name),
                );
            }
        }
        match spec.children {
            Children::None if !node.children.is_empty() => self.error(
                path,
                Some("children"),
                Code::ChildrenNotAllowed,
                format!("`{}` takes no children", spec.name),
            ),
            Children::Max(max) if node.children.len() > usize::from(max) => self.error(
                path,
                Some("children"),
                Code::ChildrenNotAllowed,
                format!("`{}` takes at most {max} child(ren), got {}", spec.name, node.children.len()),
            ),
            _ => {}
        }
    }

    fn children(
        &mut self,
        node: &Node,
        spec: Option<&ComponentSpec>,
        path: &str,
        scope: &Scope,
        depth: usize,
    ) {
        for (slot, nodes) in node.containers() {
            let mut child_scope = scope.clone();
            if matches!(node.type_.as_str(), "Show" | "Adaptive") {
                child_scope.branches.push((path.to_owned(), slot.unwrap_or("then").to_owned()));
            }
            if let Some(scope_kind) =
                slot.and_then(|name| spec.and_then(|s| s.slot(name))).and_then(|s| s.scope)
            {
                match scope_kind {
                    SlotScope::Row => child_scope.in_row = true,
                    SlotScope::Route => child_scope.in_route = true,
                }
            }
            let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
            for (index, child) in nodes.iter().enumerate() {
                let child_path = join_path(path, slot, &child.key);
                if let Some(first) = seen.insert(child.key.as_str(), index) {
                    self.error(
                        &child_path,
                        None,
                        Code::DuplicateKey,
                        format!(
                            "key `{}` is used by siblings {first} and {index}; keys are unique per slot",
                            child.key
                        ),
                    );
                }
                self.node(child, &child_path, &child_scope, depth + 1);
            }
        }
    }

    // ---- interactivity, accessibility, test ids ---------------------------------------------------------

    fn interactivity(&mut self, node: &Node, spec: &ComponentSpec, path: &str) {
        if (spec.interactive || !node.on.is_empty()) && is_auto_key(&node.key) {
            self.error(
                path,
                None,
                Code::InteractiveNeedsKey,
                format!(
                    "an interactive `{}` needs an explicit `key` (test ids and events are addressed by key)",
                    spec.name
                ),
            );
        }
    }

    fn accessibility(&mut self, node: &Node, spec: &ComponentSpec, path: &str) {
        match spec.a11y.name {
            NameRule::None => {}
            NameRule::OneOf { props, children } => {
                let named = props.iter().any(|p| node.props.get(*p).is_some_and(has_content))
                    || (children
                        && (!node.children.is_empty()
                            || node.slots.get("label").is_some_and(|n| !n.is_empty())));
                if !named {
                    let icon_only = node.props.contains_key("icon");
                    let message = if icon_only {
                        format!("an icon-only `{}` needs a `label` (its accessible name)", spec.name)
                    } else {
                        format!("`{}` needs an accessible name: set {}", spec.name, quoted(props))
                    };
                    self.error(path, None, Code::MissingName, message);
                }
            }
            NameRule::AltOrDecorative => {
                let alt = node.props.get("alt").is_some_and(has_content);
                let decorative = node.props.get("decorative") == Some(&Value::Bool(true));
                if !alt && !decorative {
                    self.error(
                        path,
                        None,
                        Code::MissingName,
                        "an `Image` needs `alt` text, or `decorative = true`",
                    );
                }
            }
        }
    }

    fn test_id(&mut self, node: &Node, path: &str, scope: &Scope) {
        let explicit = node.props.get("testid").and_then(Value::as_str).map(str::to_owned);
        let Some(id) = explicit.or_else(|| derive_test_id("", path, None)) else { return };
        let seen = self.test_ids.entry(id.clone()).or_default();
        let clash = seen
            .iter()
            .find(|(other, branches)| other != path && !exclusive(branches, &scope.branches))
            .map(|(other, _)| other.clone());
        seen.push((path.to_owned(), scope.branches.clone()));
        if let Some(first) = clash {
            self.error(
                path,
                None,
                Code::DuplicateTestId,
                format!("test id `{id}` is already used by `{first}`; set `testid` on one of them"),
            );
        }
    }
}

/// Do two nodes sit in different arms of the same `Show` or `Adaptive`?
fn exclusive(a: &[Branch], b: &[Branch]) -> bool {
    a.iter()
        .any(|(owner, arm)| b.iter().any(|(other_owner, other_arm)| owner == other_owner && arm != other_arm))
}

/// Is `value` the JSON kind `ty` describes (so its own errors are the useful ones to report)?
fn kind_fits(ty: &PropType, value: &Value) -> bool {
    match ty {
        PropType::Str | PropType::Source | PropType::Enum(_) => value.is_string(),
        PropType::Number | PropType::Int | PropType::Spacing => value.is_number(),
        PropType::Bool => value.is_boolean(),
        PropType::List(_) => value.is_array(),
        PropType::Object(_) | PropType::Frame => value.is_object(),
        PropType::Insets => value.is_object() || value.is_number(),
        PropType::Json | PropType::Union(_) => true,
    }
}

/// Unwrap `{"$lit": x}`.
fn unescape(value: &Value) -> &Value {
    value.as_object().and_then(|o| o.get("$lit")).unwrap_or(value)
}

/// A string with text, or any expression (assumed to produce text).
fn has_content(value: &Value) -> bool {
    match value {
        Value::String(text) => !text.trim().is_empty(),
        Value::Object(_) => true,
        _ => false,
    }
}

fn describe(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

fn quoted(values: &[&str]) -> String {
    values.iter().map(|v| format!("`{v}`")).collect::<Vec<_>>().join(", ")
}

fn type_name(ty: &PropType) -> String {
    match ty {
        PropType::Str | PropType::Source => "a string".to_owned(),
        PropType::Number => "a number".to_owned(),
        PropType::Int => "an integer".to_owned(),
        PropType::Bool => "a boolean".to_owned(),
        PropType::Json => "any value".to_owned(),
        PropType::Enum(def) => format!("one of {}", quoted(def.values)),
        PropType::Spacing => "a spacing from 0 to 8".to_owned(),
        PropType::Insets => "a spacing or insets".to_owned(),
        PropType::Frame => "`\"fill\"` or a frame".to_owned(),
        PropType::List(_) => "a list".to_owned(),
        PropType::Object(def) if def.name.is_empty() => "an object".to_owned(),
        PropType::Object(def) => format!("a {}", def.name),
        PropType::Union(options) => options.iter().map(type_name).collect::<Vec<_>>().join(" or "),
    }
}

#[cfg(test)]
#[path = "validate_tests.rs"]
mod tests;
