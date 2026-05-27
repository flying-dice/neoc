//! Runtime type enforcement — the Lua lowering of `lang/DYNAMIC.md` §9, with the
//! drift-point classification of `STATIC.md` §10.
//!
//! Generated code enforces declared types at runtime: deep, always-on, at drift
//! points. This module emits one reusable validator per declared type (shared,
//! never inlined per site) plus the primitive checkers and structural
//! combinators they compose from, and supplies the per-site check statements the
//! emitter lowers at each drift point.
//!
//! ## Scope
//!
//! Codegen runs on an *untyped* AST: it has every *declared* type (struct
//! fields, enum payloads, function parameters) but no *inferred* expression
//! types. So this stage covers the drift points whose types come from a
//! declaration:
//!
//! - **Construction** — a struct/newtype `.new` validates its fields against the
//!   declared field types (`DRIFT-CONSTRUCT`).
//! - **Untyped boundary params** — a `pub` free function validates each typed
//!   parameter at entry, the host being the one caller the type system cannot
//!   vouch for (`DRIFT-PARAM`, `STATIC.md` §10.4).
//!
//! Mutation (`e.f = v`) and dynamic ingress (`ext` results) are the remaining
//! drift points; their lvalue/result types need inference codegen does not carry
//! here, so they ride a later stage that threads types in through elaboration.
//!
//! ## Cross-module limit
//!
//! Validators are emitted for the types *declared in this module*. A field or
//! parameter of a type imported by `use` degrades to the identity check
//! (`__check_any`) rather than a dangling reference to a validator the other
//! module owns —
//! the deep cross-module check lands when that value is constructed in its home
//! module. Mirrors the per-module `__traits` tag (`neoc_ast::trait_index`).

use std::collections::{BTreeMap, BTreeSet};

use neoc_ast as ast;

/// A runtime-check helper emitted at module top. Only the referenced subset is
/// emitted; the order here is the emission order, and every checker that can
/// fail pulls in [`Helper::Error`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Helper {
    Error,
    Number,
    String,
    Bool,
    Fn,
    Any,
    List,
    Option,
    Result,
}

impl Helper {
    /// The Lua source for the helper. Tags are `__struct`/`__enum`/`__tag` — the
    /// runtime convention (`DYNAMIC.md` §7).
    fn source(self) -> &'static str {
        match self {
            Helper::Error => {
                "\
local function __type_error(path, expected, got)
  local actual = type(got) == \"table\" and (got.__struct or got.__enum or \"table\") or type(got)
  error(path .. \": expected \" .. expected .. \", got \" .. actual)
end"
            }
            Helper::Number => {
                "local function __check_number(v, path) if type(v) ~= \"number\" then __type_error(path, \"number\", v) end return v end"
            }
            Helper::String => {
                "local function __check_string(v, path) if type(v) ~= \"string\" then __type_error(path, \"string\", v) end return v end"
            }
            Helper::Bool => {
                "local function __check_bool(v, path) if type(v) ~= \"boolean\" then __type_error(path, \"bool\", v) end return v end"
            }
            Helper::Fn => {
                "local function __check_fn(v, path) if type(v) ~= \"function\" then __type_error(path, \"fn\", v) end return v end"
            }
            // Opaque `ext type`, bare type parameter, or imported type: no
            // inspectable shape, so the validator is the identity (`STATIC.md` §10.5).
            Helper::Any => "local function __check_any(v) return v end",
            Helper::List => {
                "\
local function __check_list(v, path, elem)
  if type(v) ~= \"table\" or v.__struct or v.__enum then __type_error(path, \"list\", v) end
  local n = 0
  for _ in pairs(v) do n = n + 1 end
  for i = 1, n do
    if v[i] == nil then __type_error(path, \"list\", v) end
    elem(v[i], path .. \"[\" .. (i - 1) .. \"]\")
  end
  return v
end"
            }
            Helper::Option => {
                "\
local function __check_option(v, path, some)
  if type(v) ~= \"table\" or v.__enum ~= \"Option\" then __type_error(path, \"Option\", v) end
  if v.__tag == \"Some\" then some(v[1], path .. \".0\")
  elseif v.__tag ~= \"None\" then __type_error(path, \"Option\", v) end
  return v
end"
            }
            Helper::Result => {
                "\
local function __check_result(v, path, ok, err)
  if type(v) ~= \"table\" or v.__enum ~= \"Result\" then __type_error(path, \"Result\", v) end
  if v.__tag == \"Ok\" then ok(v[1], path .. \".0\")
  elseif v.__tag == \"Err\" then err(v[1], path .. \".0\")
  else __type_error(path, \"Result\", v) end
  return v
end"
            }
        }
    }
}

/// Emission order for helpers: [`Helper::Error`] first (every other helper calls
/// it), then primitives, then combinators.
const HELPER_ORDER: [Helper; 9] = [
    Helper::Error,
    Helper::Number,
    Helper::String,
    Helper::Bool,
    Helper::Fn,
    Helper::Any,
    Helper::List,
    Helper::Option,
    Helper::Result,
];

/// The validator plan for a module: which types get a validator, the local type
/// set the per-site lowering resolves against, and the rendered top-of-module
/// preamble (helpers + validators). The [`Default`] is inert — no validators, an
/// empty preamble — so an emitter built without a plan enforces nothing.
#[derive(Default)]
pub(crate) struct Validation {
    /// Struct/enum names declared in this module. A type named here lowers to its
    /// validator; anything else (imported, opaque, type parameter) is identity.
    local_types: BTreeSet<String>,
    /// Types that get an emitted `__check_<name>` validator.
    validated_types: BTreeSet<String>,
    /// The helpers + validators to emit at module top, in dependency order.
    pub(crate) preamble: Vec<String>,
}

impl Validation {
    /// Whether type `name` has an emitted validator, so a `.new` may wrap its
    /// result in `__check_<name>`.
    pub(crate) fn has_validator(&self, name: &str) -> bool {
        self.validated_types.contains(name)
    }

    /// The entry checks for a `pub` free function: one per typed parameter whose
    /// type carries a runtime check, validating it against the declared type at
    /// path `fn.param` (`DRIFT-PARAM`). The receiver and unit/identity-typed
    /// parameters produce nothing.
    ///
    /// Delegates to [`Self::param_checks_into`] with a throwaway sink: the helper
    /// set was already computed in [`plan`] over these same functions, so the
    /// two passes emit identical statements and the preamble already defines
    /// every helper this output references.
    pub(crate) fn param_checks(&self, fn_name: &str, params: &[ast::Param]) -> Vec<String> {
        self.param_checks_into(fn_name, params, &mut BTreeSet::new())
    }

    /// [`Self::param_checks`], recording the helpers each check relies on into
    /// `helpers` — the single code path both the planning pre-pass and emit-time
    /// output go through, so they cannot diverge.
    fn param_checks_into(
        &self,
        fn_name: &str,
        params: &[ast::Param],
        helpers: &mut BTreeSet<Helper>,
    ) -> Vec<String> {
        let mut out = Vec::new();
        for p in params {
            if let ast::Param::Named { name, ty, .. } = p {
                let path = format!("\"{fn_name}.{name}\"");
                if let Some(stmt) = self.check_stmt(ty, name, &path, None, helpers) {
                    out.push(stmt);
                }
            }
        }
        out
    }

    /// A statement validating `value` (a Lua expression) against `ty` at `path`
    /// (a Lua expression evaluating to the path string), or `None` when `ty`
    /// carries no runtime check (`()`, an opaque/imported type, a bare type
    /// parameter — all pass-through). `self_ty` resolves `Self` inside a
    /// validator body. Records the helpers the statement relies on.
    ///
    /// A type that lowers to a named checker calls it directly; a composite calls
    /// its combinator with the element/payload validators inline — never wrapped
    /// in a closure, so the emitted check reads cleanly.
    fn check_stmt(
        &self,
        ty: &ast::Type,
        value: &str,
        path: &str,
        self_ty: Option<&str>,
        helpers: &mut BTreeSet<Helper>,
    ) -> Option<String> {
        if let Some(name) = self.named_checker(ty, self_ty, helpers) {
            return Some(format!("{name}({value}, {path})"));
        }
        match ty {
            ast::Type::List(inner) => {
                helpers.extend([Helper::List, Helper::Error]);
                let elem = self.validator_fn(inner, self_ty, helpers);
                Some(format!("__check_list({value}, {path}, {elem})"))
            }
            ast::Type::Path(p) => match composite(p) {
                Some(Composite::Option(inner)) => {
                    helpers.extend([Helper::Option, Helper::Error]);
                    let some = self.arg_validator(inner, self_ty, helpers);
                    Some(format!("__check_option({value}, {path}, {some})"))
                }
                Some(Composite::Result(ok, err)) => {
                    helpers.extend([Helper::Result, Helper::Error]);
                    let ok = self.arg_validator(ok, self_ty, helpers);
                    let err = self.arg_validator(err, self_ty, helpers);
                    Some(format!("__check_result({value}, {path}, {ok}, {err})"))
                }
                // Imported, opaque, or a bare type parameter: no check.
                None => None,
            },
            // `()` is never an observable value; `Fn`/`Self`/named types were
            // handled by `named_checker` above.
            _ => None,
        }
    }

    /// A Lua expression evaluating to a validator *function* for `ty`, for use as
    /// a combinator argument (`elem`/`some`/`ok`/`err`). A type that lowers to a
    /// named checker resolves to that name; a composite wraps its combinator in a
    /// closure so it composes without inlining; a type with no inspectable shape
    /// is the identity `__check_any`.
    fn validator_fn(
        &self,
        ty: &ast::Type,
        self_ty: Option<&str>,
        helpers: &mut BTreeSet<Helper>,
    ) -> String {
        if let Some(name) = self.named_checker(ty, self_ty, helpers) {
            return name;
        }
        match ty {
            ast::Type::List(inner) => {
                helpers.extend([Helper::List, Helper::Error]);
                let elem = self.validator_fn(inner, self_ty, helpers);
                format!("function(v, p) return __check_list(v, p, {elem}) end")
            }
            ast::Type::Path(p) => match composite(p) {
                Some(Composite::Option(inner)) => {
                    helpers.extend([Helper::Option, Helper::Error]);
                    let some = self.arg_validator(inner, self_ty, helpers);
                    format!("function(v, p) return __check_option(v, p, {some}) end")
                }
                Some(Composite::Result(ok, err)) => {
                    helpers.extend([Helper::Result, Helper::Error]);
                    let ok = self.arg_validator(ok, self_ty, helpers);
                    let err = self.arg_validator(err, self_ty, helpers);
                    format!("function(v, p) return __check_result(v, p, {ok}, {err}) end")
                }
                None => any(helpers),
            },
            _ => any(helpers),
        }
    }

    /// The bare validator-function name for a type that lowers to one — a
    /// primitive, a locally-declared type, a function, or `Self`. `None` for the
    /// composites (`[τ]`, `Option`, `Result`, handled per call shape) and for a
    /// type with no inspectable shape (handled as no-check or identity).
    fn named_checker(
        &self,
        ty: &ast::Type,
        self_ty: Option<&str>,
        helpers: &mut BTreeSet<Helper>,
    ) -> Option<String> {
        match ty {
            ast::Type::Fn { .. } => {
                helpers.extend([Helper::Fn, Helper::Error]);
                Some("__check_fn".to_string())
            }
            ast::Type::SelfType => self_ty.map(|name| {
                helpers.insert(Helper::Error);
                format!("__check_{name}")
            }),
            ast::Type::Path(p) => {
                // A primitive or a locally-declared type lowers to `__check_<name>`;
                // a primitive also pulls its own checker into the preamble.
                let name = p.segments.last()?.name.as_str();
                let primitive = match name {
                    "number" => Some(Helper::Number),
                    "string" => Some(Helper::String),
                    "bool" => Some(Helper::Bool),
                    _ => None,
                };
                if primitive.is_none() && !self.local_types.contains(name) {
                    return None;
                }
                if let Some(helper) = primitive {
                    helpers.insert(helper);
                }
                helpers.insert(Helper::Error);
                Some(format!("__check_{name}"))
            }
            _ => None,
        }
    }

    /// The validator for a generic argument that may be absent, defaulting to the
    /// identity when there is no argument to validate against.
    fn arg_validator(
        &self,
        arg: Option<&ast::Type>,
        self_ty: Option<&str>,
        helpers: &mut BTreeSet<Helper>,
    ) -> String {
        match arg {
            Some(ty) => self.validator_fn(ty, self_ty, helpers),
            None => any(helpers),
        }
    }

    /// Check statements for a set of named fields — struct fields or a struct
    /// enum-variant's — each validated deep against its declared type at
    /// `path .. ".<field>"`, prefixed by `indent`.
    fn field_checks(
        &self,
        fields: &[ast::Field],
        indent: &str,
        self_ty: Option<&str>,
        helpers: &mut BTreeSet<Helper>,
    ) -> Vec<String> {
        let mut out = Vec::new();
        for f in fields {
            let value = format!("v.{}", f.name);
            let path = format!("path .. \".{}\"", f.name);
            if let Some(stmt) = self.check_stmt(&f.ty, &value, &path, self_ty, helpers) {
                out.push(format!("{indent}{stmt}"));
            }
        }
        out
    }

    /// The body of a struct's validator: tag check, then each field deep against
    /// its declared type.
    fn struct_body(&self, s: &ast::StructDecl, helpers: &mut BTreeSet<Helper>) -> String {
        helpers.insert(Helper::Error);
        let name = &s.name;
        let mut lines = vec![
            format!("function __check_{name}(v, path)"),
            format!(
                "  if type(v) ~= \"table\" or v.__struct ~= \"{name}\" then __type_error(path, \"{name}\", v) end"
            ),
        ];
        match &s.body {
            ast::StructBody::Unit => {}
            ast::StructBody::Tuple(inner) => {
                if let Some(stmt) =
                    self.check_stmt(inner, "v[1]", "path .. \".0\"", Some(name), helpers)
                {
                    lines.push(format!("  {stmt}"));
                }
            }
            ast::StructBody::Named(fields) => {
                lines.extend(self.field_checks(fields, "  ", Some(name), helpers));
            }
        }
        lines.push("  return v".to_string());
        lines.push("end".to_string());
        lines.join("\n")
    }

    /// The body of an enum's validator: tag check, then dispatch on the variant
    /// tag and validate that variant's payload. An unrecognised tag is an error.
    fn enum_body(&self, en: &ast::EnumDecl, helpers: &mut BTreeSet<Helper>) -> String {
        helpers.insert(Helper::Error);
        let name = &en.name;
        let mut lines = vec![
            format!("function __check_{name}(v, path)"),
            format!(
                "  if type(v) ~= \"table\" or v.__enum ~= \"{name}\" then __type_error(path, \"{name}\", v) end"
            ),
            "  local t = v.__tag".to_string(),
        ];
        for (i, variant) in en.variants.iter().enumerate() {
            let kw = if i == 0 { "if" } else { "elseif" };
            lines.push(format!("  {kw} t == \"{}\" then", variant.name));
            match &variant.body {
                ast::VariantBody::Unit => {}
                ast::VariantBody::Tuple(slots) => {
                    for (slot, ty) in slots.iter().enumerate() {
                        let value = format!("v[{}]", slot + 1);
                        let path = format!("path .. \".{slot}\"");
                        if let Some(stmt) = self.check_stmt(ty, &value, &path, Some(name), helpers)
                        {
                            lines.push(format!("    {stmt}"));
                        }
                    }
                }
                ast::VariantBody::Named(fields) => {
                    lines.extend(self.field_checks(fields, "    ", Some(name), helpers));
                }
            }
        }
        lines.push(format!("  else __type_error(path, \"{name}\", v) end"));
        lines.push("  return v".to_string());
        lines.push("end".to_string());
        lines.join("\n")
    }

    /// The validator body for type `name`, or `None` if it is neither a struct
    /// nor an enum declared in this module.
    fn type_body(
        &self,
        name: &str,
        structs: &BTreeMap<&str, &ast::StructDecl>,
        enums: &BTreeMap<&str, &ast::EnumDecl>,
        helpers: &mut BTreeSet<Helper>,
    ) -> Option<String> {
        if let Some(s) = structs.get(name) {
            Some(self.struct_body(s, helpers))
        } else {
            enums.get(name).map(|e| self.enum_body(e, helpers))
        }
    }
}

/// A composite type that validates through a combinator taking its element or
/// payload validators as arguments.
enum Composite<'a> {
    Option(Option<&'a ast::Type>),
    Result(Option<&'a ast::Type>, Option<&'a ast::Type>),
}

/// Classifies a type path as `Option<τ>` / `Result<τ, ε>`, or `None` for any
/// other named type (primitive, local, imported, or type parameter).
fn composite(p: &ast::TypePath) -> Option<Composite<'_>> {
    let seg = p.segments.last()?;
    match seg.name.as_str() {
        "Option" => Some(Composite::Option(seg.args.first())),
        "Result" => Some(Composite::Result(seg.args.first(), seg.args.get(1))),
        _ => None,
    }
}

/// The identity validator `__check_any`, for a type with no inspectable shape
/// (opaque `ext type`, bare type parameter, imported type) used where a
/// combinator needs a callable element/payload validator.
fn any(helpers: &mut BTreeSet<Helper>) -> String {
    helpers.insert(Helper::Any);
    "__check_any".to_string()
}

/// Build the validator plan for `module`: index its types, close the set of
/// types needing a validator, then render the top-of-module preamble. Returns an
/// inert plan when the module has no drift point to check.
pub(crate) fn plan(module: &ast::Module) -> Validation {
    let (structs, enums) = index_types(module);
    let local_types: BTreeSet<String> = structs
        .keys()
        .chain(enums.keys())
        .map(|n| (*n).to_string())
        .collect();

    let mut v = Validation {
        local_types,
        ..Validation::default()
    };
    v.validated_types = closed_validator_set(module, &structs, &enums, &v.local_types);

    // Render every validator body once, accumulating the helpers they and the
    // `pub fn` entry checks reference, so the preamble emits exactly that subset.
    let mut helpers = BTreeSet::new();
    let mut bodies = Vec::new();
    for name in &v.validated_types {
        if let Some(body) = v.type_body(name, &structs, &enums, &mut helpers) {
            bodies.push(body);
        }
    }
    for item in &module.items {
        if let ast::ItemKind::Fn(f) = &item.kind
            && item.vis == ast::Visibility::Public
        {
            let _ = v.param_checks_into(&f.sig.name, &f.sig.params, &mut helpers);
        }
    }

    v.preamble = render_preamble(&helpers, &v.validated_types, bodies);
    v
}

/// Index the module's struct and enum declarations by name.
fn index_types(
    module: &ast::Module,
) -> (
    BTreeMap<&str, &ast::StructDecl>,
    BTreeMap<&str, &ast::EnumDecl>,
) {
    let mut structs = BTreeMap::new();
    let mut enums = BTreeMap::new();
    for item in &module.items {
        match &item.kind {
            ast::ItemKind::Struct(s) => {
                structs.insert(s.name.as_str(), s);
            }
            ast::ItemKind::Enum(e) => {
                enums.insert(e.name.as_str(), e);
            }
            _ => {}
        }
    }
    (structs, enums)
}

/// The set of types needing a validator: every struct with a `.new` (it
/// validates itself at construction) and every type reaching a `pub` parameter
/// (checked at entry), closed over the types each validator's members reference.
fn closed_validator_set(
    module: &ast::Module,
    structs: &BTreeMap<&str, &ast::StructDecl>,
    enums: &BTreeMap<&str, &ast::EnumDecl>,
    local_types: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut worklist: Vec<String> = structs
        .values()
        .filter(|s| !matches!(s.body, ast::StructBody::Unit))
        .map(|s| s.name.clone())
        .collect();
    for item in &module.items {
        if let ast::ItemKind::Fn(f) = &item.kind
            && item.vis == ast::Visibility::Public
        {
            for p in &f.sig.params {
                if let ast::Param::Named { ty, .. } = p {
                    collect_local_refs(ty, local_types, &mut worklist);
                }
            }
        }
    }

    let mut validated = BTreeSet::new();
    while let Some(name) = worklist.pop() {
        if !validated.insert(name.clone()) {
            continue;
        }
        if let Some(s) = structs.get(name.as_str()) {
            for ty in struct_member_types(s) {
                collect_local_refs(ty, local_types, &mut worklist);
            }
        } else if let Some(e) = enums.get(name.as_str()) {
            for ty in enum_member_types(e) {
                collect_local_refs(ty, local_types, &mut worklist);
            }
        }
    }
    validated
}

/// Assemble the preamble: the referenced helpers in dependency order, a single
/// forward declaration of every validator local (so mutually recursive types
/// resolve), then the validator bodies. Empty when nothing is checked.
fn render_preamble(
    helpers: &BTreeSet<Helper>,
    validated_types: &BTreeSet<String>,
    bodies: Vec<String>,
) -> Vec<String> {
    if helpers.is_empty() && bodies.is_empty() {
        return Vec::new();
    }
    let mut preamble: Vec<String> = HELPER_ORDER
        .into_iter()
        .filter(|h| helpers.contains(h))
        .map(|h| h.source().to_string())
        .collect();
    if !validated_types.is_empty() {
        let names: Vec<String> = validated_types
            .iter()
            .map(|n| format!("__check_{n}"))
            .collect();
        preamble.push(format!("local {}", names.join(", ")));
    }
    preamble.extend(bodies);
    preamble
}

/// The declared types of a struct's members (newtype slot or named fields).
fn struct_member_types(s: &ast::StructDecl) -> Vec<&ast::Type> {
    match &s.body {
        ast::StructBody::Unit => Vec::new(),
        ast::StructBody::Tuple(ty) => vec![ty],
        ast::StructBody::Named(fields) => fields.iter().map(|f| &f.ty).collect(),
    }
}

/// The declared types carried by an enum's variant payloads.
fn enum_member_types(e: &ast::EnumDecl) -> Vec<&ast::Type> {
    let mut out = Vec::new();
    for variant in &e.variants {
        match &variant.body {
            ast::VariantBody::Unit => {}
            ast::VariantBody::Tuple(slots) => out.extend(slots.iter()),
            ast::VariantBody::Named(fields) => out.extend(fields.iter().map(|f| &f.ty)),
        }
    }
    out
}

/// Adds the names of every locally-declared type referenced by `ty` (itself, or
/// nested inside a list, generic argument, or function type) to `out`.
fn collect_local_refs(ty: &ast::Type, local_types: &BTreeSet<String>, out: &mut Vec<String>) {
    match ty {
        ast::Type::Unit | ast::Type::SelfType => {}
        ast::Type::List(inner) => collect_local_refs(inner, local_types, out),
        ast::Type::Fn { params, ret } => {
            for p in params {
                collect_local_refs(p, local_types, out);
            }
            if let Some(r) = ret {
                collect_local_refs(r, local_types, out);
            }
        }
        ast::Type::Path(p) => {
            if let Some(seg) = p.segments.last() {
                if local_types.contains(&seg.name) {
                    out.push(seg.name.clone());
                }
                for arg in &seg.args {
                    collect_local_refs(arg, local_types, out);
                }
            }
        }
    }
}
