//! The attributes neoc-vm's modules declare their Lua surface with.
//!
//! A function a script calls is implemented once, in Rust, and what the declarations say of it (its name, its
//! parameters and their types, its doc) is read from that implementation instead of being written again beside
//! it:
//!
//! ```ignore
//! /// Decode a standard-alphabet base64 string back to raw bytes.
//! #[lua_fn(returns = "Result<string, string>")]
//! fn decode(lua: &Lua, input: mlua::LuaString) -> mlua::Result<(Value, Value)> { ... }
//!
//! m.declared(decode::lua(), lua.create_function(decode)?)?;
//! ```
//!
//! [`lua_fn`] is for a function, [`lua_userdata`] for the methods and fields of an `impl UserData` block, and
//! [`lua_module`] for the closures a module's `declare` function registers in place.
//!
//! What is read: the name (the function's, or the string a method is added under), each parameter's name and
//! its type through `DeclType` (`String` is `string`, `Option<i64>` is `number?`), and the `///` lines as the doc.
//! What Rust does not say is said once, on the spot: a return whose shape is made in the body (`(value, err)`),
//! the type of a parameter that is a `Value` or a `Table` in Rust and something narrower to a script
//! (`types(key = "string | number")`), a parameter a script knows under another name (`names(s = "input")`), and
//! the whole list where the function takes its arguments as they come (`takes("path: string", "...: LuaValue")`).

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::{ToTokens, quote};
use syn::spanned::Spanned;
use syn::{
    Attribute, Expr, ExprClosure, FnArg, GenericArgument, Ident, ImplItem, ItemFn, ItemImpl, Lit,
    LitStr, Pat, PathArguments, ReturnType, Stmt, Token, Type,
};

/// Where neoc-vm keeps what the generated code names.
fn facade() -> Tokens {
    quote!(crate::lua::facade)
}

/// The lines of the `///` comments of `attrs`, joined: a doc as the declarations keep it.
fn doc_of(attrs: &[Attribute]) -> String {
    let mut lines = Vec::new();
    for attr in attrs.iter().filter(|a| a.path().is_ident("doc")) {
        if let syn::Meta::NameValue(pair) = &attr.meta
            && let Expr::Lit(lit) = &pair.value
            && let Lit::Str(text) = &lit.lit
        {
            let text = text.value();
            lines.push(text.strip_prefix(' ').unwrap_or(&text).to_owned());
        }
    }
    lines.join("\n")
}

/// What is said by hand of a function or a member: in `#[lua_fn(...)]`, or in `#[lua(...)]` above a member.
#[derive(Default)]
struct Said {
    /// `name = "..."`: the name a script uses, when it is not the Rust one.
    name: Option<LitStr>,
    /// `"..."` or `returns = "..."`: the recorded return (of a field, its type).
    returns: Option<LitStr>,
    /// `types(key = "string | number")`: the recorded type of a parameter, by its Rust name.
    types: Vec<(Ident, LitStr)>,
    /// `names(s = "input")`: the name a script knows a parameter by, by its Rust name.
    names: Vec<(Ident, LitStr)>,
    /// `takes("path: string", "...: LuaValue")`: the whole parameter list, when Rust's says nothing of it.
    takes: Option<Vec<LitStr>>,
    /// `skip`: a member a script does not name.
    skip: bool,
}

impl Said {
    fn read(&mut self, input: syn::parse::ParseStream) -> syn::Result<()> {
        while !input.is_empty() {
            if input.peek(LitStr) {
                self.returns = Some(input.parse()?);
            } else {
                let word: Ident = input.parse()?;
                match word.to_string().as_str() {
                    "skip" => self.skip = true,
                    "name" => {
                        input.parse::<Token![=]>()?;
                        self.name = Some(input.parse()?);
                    }
                    "returns" => {
                        input.parse::<Token![=]>()?;
                        self.returns = Some(input.parse()?);
                    }
                    "types" | "names" => {
                        let inner;
                        syn::parenthesized!(inner in input);
                        let list = if word == "types" {
                            &mut self.types
                        } else {
                            &mut self.names
                        };
                        while !inner.is_empty() {
                            let binding: Ident = inner.parse()?;
                            inner.parse::<Token![=]>()?;
                            list.push((binding, inner.parse()?));
                            if !inner.is_empty() {
                                inner.parse::<Token![,]>()?;
                            }
                        }
                    }
                    "takes" => {
                        let inner;
                        syn::parenthesized!(inner in input);
                        let list = inner.parse_terminated(
                            syn::parse::ParseBuffer::parse::<LitStr>,
                            Token![,],
                        )?;
                        self.takes = Some(list.into_iter().collect());
                    }
                    _ => {
                        return Err(syn::Error::new(
                            word.span(),
                            "expected a return type as a string, `name = \"...\"`, `returns = \"...\"`, \
                             `types(...)`, `names(...)`, `takes(...)` or `skip`",
                        ));
                    }
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(())
    }

    /// Reads the `#[lua(...)]` attributes of `attrs` and takes them out (they mean nothing to the compiler).
    fn above(attrs: &mut Vec<Attribute>) -> syn::Result<Self> {
        let mut said = Self::default();
        for attr in attrs.iter().filter(|a| a.path().is_ident("lua")) {
            // a bare `#[lua]` says only that the statement is one to read
            if !matches!(attr.meta, syn::Meta::Path(_)) {
                attr.parse_args_with(|input: syn::parse::ParseStream| said.read(input))?;
            }
        }
        attrs.retain(|a| !a.path().is_ident("lua"));
        Ok(said)
    }

    /// The parameters a script passes, as expressions of `(String, String)`: the list said by hand, or each
    /// Rust parameter under its name with the type it records.
    fn params<'p>(
        &self,
        inputs: impl Iterator<Item = (&'p Pat, Option<&'p Type>)>,
    ) -> syn::Result<Vec<Tokens>> {
        if let Some(takes) = &self.takes {
            return takes
                .iter()
                .map(|pair| {
                    let text = pair.value();
                    let Some((name, ty)) = text.split_once(':') else {
                        return Err(syn::Error::new(pair.span(), "expected `\"name: type\"`"));
                    };
                    let (name, ty) = (name.trim(), ty.trim());
                    Ok(quote!((::std::string::String::from(#name), ::std::string::String::from(#ty))))
                })
                .collect();
        }
        let mut found = Vec::new();
        for (pat, ty) in inputs {
            self.param(pat, ty, &mut found)?;
        }
        Ok(found)
    }

    /// What one Rust parameter is to a script: nothing for `_: ()`, one parameter for `name: T`, one each for
    /// `(a, b): (A, B)`, and any number (`...`) for `rest: Variadic<T>`.
    fn param(&self, pat: &Pat, ty: Option<&Type>, found: &mut Vec<Tokens>) -> syn::Result<()> {
        match pat {
            Pat::Type(typed) => return self.param(&typed.pat, Some(&typed.ty), found),
            Pat::Tuple(parts) => {
                let types: Vec<Option<&Type>> = match ty {
                    Some(Type::Tuple(tuple)) if tuple.elems.len() == parts.elems.len() => {
                        tuple.elems.iter().map(Some).collect()
                    }
                    None => parts.elems.iter().map(|_| None).collect(),
                    Some(other) => {
                        return Err(syn::Error::new(
                            other.span(),
                            "a tuple of parameters needs a tuple of as many types",
                        ));
                    }
                };
                for (part, ty) in parts.elems.iter().zip(types) {
                    self.param(part, ty, found)?;
                }
                return Ok(());
            }
            _ => {}
        }
        if ty.is_some_and(is_unit) {
            return Ok(());
        }
        let Pat::Ident(binding) = pat else {
            return Err(syn::Error::new(
                pat.span(),
                "a parameter a script passes needs a name: name it, or say the list with `takes(...)`",
            ));
        };
        let binding = &binding.ident;
        let name = if let Some((_, name)) = self.names.iter().find(|(of, _)| of == binding) {
            name.value()
        } else {
            let name = binding.to_string();
            let name = name.strip_prefix("r#").unwrap_or(&name);
            name.strip_prefix('_').unwrap_or(name).to_owned()
        };
        let said = self
            .types
            .iter()
            .find(|(of, _)| of == binding)
            .map(|(_, ty)| ty);
        let variadic = ty.and_then(|ty| generic_of(ty, "Variadic"));
        let recorded = match (said, variadic.or(ty)) {
            (Some(text), _) => quote!(::std::string::String::from(#text)),
            (None, Some(ty)) => {
                let facade = facade();
                quote!(<#ty as #facade::DeclType>::decl_type())
            }
            (None, None) => {
                return Err(syn::Error::new(
                    pat.span(),
                    "a parameter a script passes needs its type: write it, or say it with `types(...)`",
                ));
            }
        };
        let name = if variadic.is_some() {
            "...".to_owned()
        } else {
            name
        };
        found.push(quote!((::std::string::String::from(#name), #recorded)));
        Ok(())
    }
}

/// `T` of `Name<T>` when the last word of the type's path is `name`.
fn generic_of<'t>(ty: &'t Type, name: &str) -> Option<&'t Type> {
    let Type::Path(path) = ty else { return None };
    let last = path.path.segments.last()?;
    if last.ident != name {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    match args.args.first()? {
        GenericArgument::Type(inner) => Some(inner),
        _ => None,
    }
}

fn is_unit(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
}

/// Whether a parameter is the `&Lua` every bound function takes first.
fn is_lua(ty: &Type) -> bool {
    let named = |ty: &Type| matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|s| s.ident == "Lua"));
    // `&Lua`, or `Lua` itself for an async function
    match ty {
        Type::Reference(reference) => named(&reference.elem),
        other => named(other),
    }
}

/// The recorded return of a function: what was said, or the `DeclType` of `T` in `mlua::Result<T>`.
fn returns_of(said: Option<&LitStr>, output: &ReturnType) -> syn::Result<Tokens> {
    if let Some(text) = said {
        return Ok(quote!(::std::string::String::from(#text)));
    }
    let facade = facade();
    match output {
        ReturnType::Default => Ok(quote!(::std::string::String::from("()"))),
        ReturnType::Type(_, ty) => match generic_of(ty, "Result") {
            Some(value) => Ok(quote!(<#value as #facade::DeclType>::decl_type())),
            None => Err(syn::Error::new(
                ty.span(),
                "the return is not `mlua::Result<T>`: say what a script gets with `returns = \"...\"`",
            )),
        },
    }
}

fn lua_fn_impl(args: TokenStream, mut function: ItemFn) -> syn::Result<Tokens> {
    let mut said = Said::default();
    syn::parse::Parser::parse(|input: syn::parse::ParseStream| said.read(input), args)?;
    if said.skip {
        return Err(syn::Error::new(
            Span::call_site(),
            "`skip` is for a member of a userdata type",
        ));
    }

    let ident = function.sig.ident.clone();
    let name = said
        .name
        .as_ref()
        .map_or_else(|| ident.to_string(), LitStr::value);
    let doc = doc_of(&function.attrs);
    // the doc is the script's, not the Rust function's: it is kept in the declaration, not as rustdoc
    function.attrs.retain(|a| !a.path().is_ident("doc"));
    let mut inputs = Vec::new();
    for (index, input) in function.sig.inputs.iter().enumerate() {
        let FnArg::Typed(typed) = input else {
            return Err(syn::Error::new(
                input.span(),
                "`lua_fn` is for a free function",
            ));
        };
        if index == 0 && is_lua(&typed.ty) {
            continue;
        }
        inputs.push((&*typed.pat, Some(&*typed.ty)));
    }
    let params = said.params(inputs.into_iter())?;
    let returns = returns_of(said.returns.as_ref(), &function.sig.output)?;
    let facade = facade();
    let visibility = &function.vis;
    Ok(quote! {
        #function

        #[doc(hidden)]
        #[allow(non_snake_case, clippy::wildcard_imports, unused_imports)]
        #visibility mod #ident {
            use super::*;

            /// What the declarations say of the function of the same name: read from it by `lua_fn`.
            pub(crate) fn lua() -> #facade::FnDecl {
                #facade::FnDecl {
                    name: ::std::string::String::from(#name),
                    params: ::std::vec![#(#params),*],
                    ret: #returns,
                    doc: ::std::string::String::from(#doc),
                }
            }
        }
    })
}

/// Records what the declarations say of a function from the function itself.
///
/// Beside the function it writes a module of the same name with one function, `lua()`, that returns its
/// `FnDecl`: hand both to the module (`m.declared(decode::lua(), lua.create_function(decode)?)`).
///
/// - The name is the function's, or `name = "..."`.
/// - A leading `&Lua` is not a parameter. Every other parameter is one, under its own name, with the type its
///   Rust type records (`DeclType`); `(a, b): (A, B)` is two, `_: ()` none, `rest: Variadic<T>` any number.
///   `types(key = "...")` says a parameter's type by hand, `names(s = "...")` its name, and
///   `takes("a: string", ...)` the whole list.
/// - The return is the `T` of `mlua::Result<T>`, or `returns = "..."`.
/// - The doc is the `///` lines: they are the script's, and are not kept as the Rust function's.
#[proc_macro_attribute]
pub fn lua_fn(args: TokenStream, item: TokenStream) -> TokenStream {
    let function = syn::parse_macro_input!(item as ItemFn);
    lua_fn_impl(args, function)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// How a member of a userdata type is added, by the name of what adds it.
enum Member {
    Field,
    Method,
    Function,
}

/// `add_field_method_get` adds a field, `add_function` a function with no receiver, anything with `meta` in its
/// name nothing a script names, and every other `*method*` (`add_method_mut`, `async_method`) a method.
fn member_kind(adder: &str) -> Option<Member> {
    if adder.contains("meta") {
        None
    } else if adder.contains("field") {
        Some(Member::Field)
    } else if adder.contains("function") {
        Some(Member::Function)
    } else if adder.contains("method") {
        Some(Member::Method)
    } else {
        None
    }
}

/// The attributes of a statement that is a call (with or without `?`), what is called, and its arguments.
fn call_of(stmt: &mut Stmt) -> Option<(&mut Vec<Attribute>, String, Vec<&mut Expr>)> {
    let Stmt::Expr(expr, _) = stmt else {
        return None;
    };
    let expr = if matches!(expr, Expr::Try(tried) if tried.attrs.is_empty()) {
        let Expr::Try(tried) = expr else { return None };
        &mut *tried.expr
    } else {
        expr
    };
    match expr {
        Expr::MethodCall(call) => Some((
            &mut call.attrs,
            call.method.to_string(),
            call.args.iter_mut().collect(),
        )),
        Expr::Call(call) => {
            let Expr::Path(path) = &*call.func else {
                return None;
            };
            let adder = path.path.segments.last()?.ident.to_string();
            Some((&mut call.attrs, adder, call.args.iter_mut().collect()))
        }
        _ => None,
    }
}

/// The record of one statement of `add_fields` or `add_methods`, as statements on `spec`: nothing for a
/// statement that adds no member a script names.
fn member(stmt: &mut Stmt) -> syn::Result<Option<Tokens>> {
    let span = stmt.span();
    let Some((attrs, adder, args)) = call_of(stmt) else {
        return Ok(None);
    };
    let doc = doc_of(attrs);
    attrs.retain(|a| !a.path().is_ident("doc"));
    let said = Said::above(attrs)?;
    let Some(kind) = member_kind(&adder) else {
        return Ok(None);
    };
    let mut name = None;
    let mut closure: Option<&ExprClosure> = None;
    for arg in args {
        match arg {
            Expr::Lit(lit) => {
                if let (None, Lit::Str(text)) = (&name, &lit.lit) {
                    name = Some(text.clone());
                }
            }
            Expr::Closure(found) => closure = Some(found),
            _ => {}
        }
    }
    let (Some(name), false) = (said.name.clone().or(name), said.skip) else {
        return Ok(None);
    };
    let Some(ty) = &said.returns else {
        return Err(syn::Error::new(
            span,
            "say what a script gets from this member: `#[lua(\"...\")]` above it (or `#[lua(skip)]`)",
        ));
    };
    // the closure's own parameters: `lua`, then `this` for a method, then what a script passes
    let own = match kind {
        Member::Field => return Ok(Some(quote!(spec.field(#name, #ty, #doc);))),
        Member::Method => 2,
        Member::Function => 1,
    };
    let inputs = closure
        .into_iter()
        .flat_map(|c| c.inputs.iter().skip(own))
        .map(|pat| (pat, None));
    let params = said.params(inputs)?;
    let record = if matches!(kind, Member::Function) {
        quote!(function)
    } else {
        quote!(method)
    };
    Ok(Some(quote! {{
        let params: ::std::vec::Vec<(::std::string::String, ::std::string::String)> = ::std::vec![#(#params),*];
        let params: ::std::vec::Vec<(&str, &str)> = params.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
        spec.#record(#name, &params, #ty, #doc);
    }}))
}

fn lua_userdata_impl(mut block: ItemImpl) -> syn::Result<Tokens> {
    let mut records = Vec::new();
    for item in &mut block.items {
        let ImplItem::Fn(function) = item else {
            continue;
        };
        let name = function.sig.ident.to_string();
        if name != "add_fields" && name != "add_methods" {
            continue;
        }
        for stmt in &mut function.block.stmts {
            records.extend(member(stmt)?);
        }
    }
    let ty = &block.self_ty;
    let (generics, _, where_clause) = block.generics.split_for_impl();
    let facade = facade();
    Ok(quote! {
        #block

        impl #generics #ty #where_clause {
            /// What the declarations say of this type's members: read from its `impl UserData` by
            /// `lua_userdata`.
            pub(crate) fn lua(spec: &mut impl #facade::Members) {
                #(#records)*
            }
        }
    })
}

/// Records what the declarations say of a userdata type's members from its `impl UserData` block.
///
/// It writes `Type::lua(spec)`, which records every member the block adds at the top of `add_fields` and
/// `add_methods`, in order: hand it to the module (`m.userdata("Version", VersionHandle::lua)`).
///
/// - The name is the string the member is added under.
/// - `#[lua("...")]` above the statement says what a script gets: a field's type, a method's return.
/// - A method's parameters are the closure's after `lua` and `this` (a function's, after `lua`), read as
///   `lua_fn` reads them, with `types(...)`, `names(...)` and `takes(...)` in the same `#[lua(...)]`.
/// - The doc is the `///` lines above the statement.
/// - A metamethod is not a member a script names; `#[lua(skip)]` leaves another one out.
#[proc_macro_attribute]
pub fn lua_userdata(args: TokenStream, item: TokenStream) -> TokenStream {
    if !args.is_empty() {
        return syn::Error::new(Span::call_site(), "`lua_userdata` takes no arguments")
            .into_compile_error()
            .into();
    }
    let block = syn::parse_macro_input!(item as ItemImpl);
    lua_userdata_impl(block)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// The first closure in an expression: the one a registration is given, however it is wrapped
/// (`lua.create_function(|lua, key: String| ...)?`, `async_function(lua, Role::Timer, |_, secs: f64| ...)`).
fn closure_in(expr: &Expr) -> Option<&ExprClosure> {
    struct First<'e>(Option<&'e ExprClosure>);
    impl<'e> syn::visit::Visit<'e> for First<'e> {
        fn visit_expr_closure(&mut self, closure: &'e ExprClosure) {
            self.0.get_or_insert(closure);
        }
    }
    let mut first = First(None);
    syn::visit::Visit::visit_expr(&mut first, expr);
    first.0
}

/// The registrations that take the parameter list and the doc themselves: `lua_module` writes both in.
const REGISTRATIONS: &[&str] = &[
    "func",
    "try_func",
    "try_func_raising",
    "async_func",
    "try_async_func",
    "constructor",
    "try_constructor",
];

/// The call a statement is, when it is one (`m.func(...)?;`), with the statement's attributes on it: syn
/// leaves them on the outermost expression, which is the `?`.
fn call_in(stmt: &mut Stmt) -> Option<&mut syn::ExprMethodCall> {
    let Stmt::Expr(expr, _) = stmt else {
        return None;
    };
    let call = match expr {
        Expr::Try(tried) => {
            let above = std::mem::take(&mut tried.attrs);
            let inner = &mut *tried.expr;
            if let Expr::MethodCall(call) = inner {
                call.attrs.extend(above);
            }
            inner
        }
        other => other,
    };
    match call {
        Expr::MethodCall(call) => Some(call),
        _ => None,
    }
}

/// What a marked statement says: the name it registers under, the attributes, the doc, and what it is given.
struct Marked {
    name: LitStr,
    said: Said,
    doc: String,
    kind: String,
    receiver: Tokens,
    closure: Option<ExprClosure>,
    /// The function, when it is given by its path and says its own parameters and doc (`#[lua_fn]`).
    path: Option<syn::ExprPath>,
}

impl Marked {
    /// Reads a statement and takes the marks out of it; `None` when it is not marked.
    fn read(stmt: &mut Stmt) -> syn::Result<Option<Self>> {
        let span = stmt.span();
        let Some(call) = call_in(stmt) else {
            return Ok(None);
        };
        if !call.attrs.iter().any(|a| a.path().is_ident("lua")) {
            return Ok(None);
        }
        let doc = doc_of(&call.attrs);
        call.attrs.retain(|a| !a.path().is_ident("doc"));
        let said = Said::above(&mut call.attrs)?;
        let Some(Expr::Lit(syn::ExprLit {
            lit: Lit::Str(name),
            ..
        })) = call.args.first()
        else {
            return Err(syn::Error::new(
                span,
                "the first argument is the name, as a string",
            ));
        };
        let closure = call.args.iter().skip(1).find_map(closure_in).cloned();
        let path = match call.args.iter().nth(1) {
            Some(Expr::Path(path)) if said.takes.is_none() => Some(path.clone()),
            _ => None,
        };
        Ok(Some(Self {
            name: name.clone(),
            said,
            doc,
            kind: call.method.to_string(),
            receiver: call.receiver.to_token_stream(),
            closure,
            path,
        }))
    }

    /// The parameters a script passes: the closure's after `lua`, or the list said by hand.
    fn params(&self, span: Span) -> syn::Result<Vec<Tokens>> {
        if self.closure.is_none() && self.said.takes.is_none() {
            return Err(syn::Error::new(
                span,
                "no closure to read the parameters from: say them with `takes(...)`",
            ));
        }
        let inputs = self
            .closure
            .iter()
            .flat_map(|c| c.inputs.iter().skip(1))
            .map(|pat| (pat, None));
        self.said.params(inputs)
    }
}

/// Writes the parameter list and the doc into a registration, after the name.
fn with_params(stmt: &mut Stmt, params: &Expr, doc: &Expr) {
    if let Some(call) = call_in(stmt) {
        call.args.insert(1, params.clone());
        call.args.insert(2, doc.clone());
    }
}

/// One statement of a `declare` function marked `#[lua(...)]`, with what the declarations say of it written
/// in: the parameters and the doc as arguments of a registration, or a `record` after a `set`.
fn registered(stmt: &mut Stmt) -> syn::Result<Option<Tokens>> {
    let span = stmt.span();
    let Some(marked) = Marked::read(stmt)? else {
        return Ok(None);
    };
    let facade = facade();
    let (name, doc, receiver) = (&marked.name, &marked.doc, &marked.receiver);
    if marked.kind == "set" {
        let Some(returns) = &marked.said.returns else {
            return Err(syn::Error::new(
                span,
                "say what a script gets from this function: `#[lua(\"...\")]`",
            ));
        };
        let params = marked.params(span)?;
        return Ok(Some(quote! {
            #stmt
            #receiver.record(#facade::FnDecl {
                name: ::std::string::String::from(#name),
                params: ::std::vec![#(#params),*],
                ret: ::std::string::String::from(#returns),
                doc: ::std::string::String::from(#doc),
            });
        }));
    }
    if !REGISTRATIONS.contains(&marked.kind.as_str()) {
        return Err(syn::Error::new(
            span,
            "`#[lua]` is for a `set` or a registration (`func`, `try_func`, ..., `constructor`)",
        ));
    }
    if let Some(returns) = &marked.said.returns {
        return Err(syn::Error::new(
            returns.span(),
            "the return of a registration is its function's: it is not said by hand",
        ));
    }
    if let Some(path) = &marked.path {
        if !doc.is_empty() {
            return Err(syn::Error::new(
                span,
                "the doc of a function given by its path is the function's",
            ));
        }
        with_params(
            stmt,
            &syn::parse_quote!(&__lua_params),
            &syn::parse_quote!(&__lua_decl.doc),
        );
        return Ok(Some(quote! {
            {
                let __lua_decl = #path::lua();
                let __lua_params = __lua_decl.borrowed();
                #stmt
            }
        }));
    }
    let params = marked.params(span)?;
    with_params(
        stmt,
        &syn::parse_quote!(&__lua_params),
        &syn::parse_quote!(#doc),
    );
    Ok(Some(quote! {
        {
            let __lua_owned: ::std::vec::Vec<(::std::string::String, ::std::string::String)> = ::std::vec![#(#params),*];
            let __lua_params: ::std::vec::Vec<(&str, &str)> = __lua_owned.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
            #stmt
        }
    }))
}

/// Rewrites the statements of a block that are marked, and those of the closures given to `type_`.
fn register_block(block: &mut syn::Block) -> syn::Result<()> {
    let mut out = Vec::with_capacity(block.stmts.len());
    for mut stmt in std::mem::take(&mut block.stmts) {
        if let Some(tokens) = registered(&mut stmt)? {
            let wrapped: syn::Block = syn::parse2(quote!({ #tokens }))?;
            out.extend(wrapped.stmts);
        } else {
            if let Stmt::Expr(expr, _) = &mut stmt {
                nested(expr)?;
            }
            out.push(stmt);
        }
    }
    block.stmts = out;
    Ok(())
}

/// The body of a closure given to `type_` is a list of registrations too (`t.constructor(...)`).
fn nested(expr: &mut Expr) -> syn::Result<()> {
    let call = match expr {
        Expr::Try(tried) => &mut *tried.expr,
        other => other,
    };
    let Expr::MethodCall(call) = call else {
        return Ok(());
    };
    if call.method != "type_" {
        return Ok(());
    }
    for arg in &mut call.args {
        if let Expr::Closure(closure) = arg
            && let Expr::Block(body) = &mut *closure.body
        {
            register_block(&mut body.block)?;
        }
    }
    Ok(())
}

fn lua_module_impl(mut function: ItemFn) -> syn::Result<Tokens> {
    register_block(&mut function.block)?;
    Ok(function.to_token_stream())
}

/// Reads what the declarations say of the closures a module's `declare` function registers in place.
///
/// A statement of the function marked `#[lua(...)]` is one of:
///
/// - a registration, `m.func("name", |lua, input: String| ...)?` (and `try_func`, `try_func_raising`,
///   `async_func`, `try_async_func`, a type's `constructor` and `try_constructor`): the parameter list and the
///   doc are written into the call, so neither is an argument in the source. Given a function by its path
///   (`m.try_func("parse", parse)?`), both are that function's own (`#[lua_fn]`);
/// - `m.set("name", <a function made from a closure>)?`: the function is recorded after it, with the return
///   `#[lua("...")]` says.
///
/// The parameters are the closure's after `lua`, read as `lua_fn` reads them (`types(...)`, `names(...)`,
/// `takes(...)`), and the doc is the `///` lines above the statement.
#[proc_macro_attribute]
pub fn lua_module(args: TokenStream, item: TokenStream) -> TokenStream {
    if !args.is_empty() {
        return syn::Error::new(Span::call_site(), "`lua_module` takes no arguments")
            .into_compile_error()
            .into();
    }
    let function = syn::parse_macro_input!(item as ItemFn);
    lua_module_impl(function)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
