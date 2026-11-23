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
//! [`lua_fn`] is for a function, [`lua_userdata`] for the methods and fields of an `impl UserData` block.
//!
//! What is read: the name (the function's, or the string a method is added under), each parameter's name and
//! its type through `DeclType` (`String` is `string`, `Option<i64>` is `number?`), and the `///` lines as the doc.
//! What Rust does not say is said once, on the spot: a return whose shape is made in the body (`(value, err)`),
//! the type of a parameter that is a `Value` or a `Table` in Rust and something narrower to a script
//! (`types(key = "string | number")`), a parameter a script knows under another name (`names(s = "input")`), and
//! the whole list where the function takes its arguments as they come (`takes("path: string", "...: LuaValue")`).

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::quote;
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
            attr.parse_args_with(|input: syn::parse::ParseStream| said.read(input))?;
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
    matches!(ty, Type::Reference(reference)
        if matches!(&*reference.elem, Type::Path(path) if path.path.segments.last().is_some_and(|s| s.ident == "Lua")))
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
