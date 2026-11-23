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
//! The macros write paths into neoc-vm's facade (`crate::lua::facade`), so they are for neoc-vm's own modules.
//!
//! [`lua_fn`] is for a function, [`lua_userdata`] for the methods and fields of an `impl UserData` block
//! ([`lua_members`] for a type that is a plain table a function builds), and
//! [`lua_module`] for the closures a module's `declare` function registers in place.
//!
//! What is read: the name (the function's, or the string a method is added under), each parameter's name and
//! its type through `DeclType` (`String` is `string`, `Option<i64>` is `number?`), and the `///` lines as the doc.
//! What Rust does not say is said once, on the spot: a return whose shape is made in the body (`(value, err)`),
//! the type of a parameter that is a `Value` or a `Table` in Rust and something narrower to a script
//! (`types(key = "string | number")`), a parameter a script knows under another name (`names(s = "input")`), and
//! the whole list where the function takes its arguments as they come (`takes("path: string", "...: LuaValue")`).
//!
//! # The words
//!
//! Every word goes in `#[lua_fn(...)]`, or in `#[lua(...)]` above a statement. A word that means nothing where
//! it is said is refused there, and so is one said twice.
//!
//! | Word | Says | Where |
//! | --- | --- | --- |
//! | `"T"` or `returns = EXPR` | what a script gets: a return, a field's or a constant's type. The first is a literal, the second any expression that gives a `&str` or a `String` (`returns = RESULT`, `returns = &format!(...)`) | everywhere but a registration, which reads the return itself |
//! | `name = "..."` | the name a script calls it by | a function, a member (it must be said for one added out of sight, and replaces the name one is added under) |
//! | `types(binding = EXPR)` | the type of the parameter bound as `binding` in Rust | anything with parameters |
//! | `names(binding = "...")` | the name a script knows that parameter under | anything with parameters |
//! | `takes("a: T", name = EXPR)` | the whole parameter list: each entry is `"name: type"`, or a name with an expression that gives its type; `takes()` is no parameters | anything with parameters |
//! | `doc = EXPR` | the doc, where it is put together and not written (never beside `///` lines) | everywhere but a registration given a function by its path |
//! | `doc_with = path` | a `fn(&str) -> String` the doc is given to, for an ending several docs share | the same |
//! | `registered` | that the function is handed to a registration, which reads the return itself | a function |
//! | `method`, `function`, `field` | what kind of member this is, where the call that adds it does not say | a member |
//! | `skip` | that a script does not name this member | a member |
//! | `constant` | that the statement sets a value and not a function | a `set` in a `declare` function |
//! | `on(m, "Type")` | that the constant is set on the sub-table of the type `Type` of the module `m` (`Duration.ZERO`) | a constant |

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

/// One parameter of a list said whole: `"path: string"`, or `opts = OPTIONS` for a type that is put together.
enum Taken {
    Text(LitStr),
    Named(Ident, Box<Expr>),
}

impl syn::parse::Parse for Taken {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        if input.peek(LitStr) {
            return Ok(Self::Text(input.parse()?));
        }
        let name = input.parse()?;
        input.parse::<Token![=]>()?;
        Ok(Self::Named(name, Box::new(input.parse()?)))
    }
}

/// What is said by hand of a function or a member: in `#[lua_fn(...)]`, or in `#[lua(...)]` above a member.
#[derive(Default)]
struct Said {
    /// `name = "..."`: the name a script uses, when it is not the Rust one.
    name: Option<LitStr>,
    /// `"..."` or `returns = "..."`: the recorded return (of a field, its type).
    returns: Option<Expr>,
    /// `types(key = "string | number")`: the recorded type of a parameter, by its Rust name.
    types: Vec<(Ident, Expr)>,
    /// `names(s = "input")`: the name a script knows a parameter by, by its Rust name.
    names: Vec<(Ident, LitStr)>,
    /// `takes("path: string", "...: LuaValue")`: the whole parameter list, when Rust's says nothing of it.
    takes: Option<Vec<Taken>>,
    /// `method`, `function` or `field`: what a member is to a script, when what adds it does not say.
    kind: Option<Member>,
    /// `skip`: a member a script does not name.
    skip: bool,
    /// `doc_with = path`: a function the doc is given to, for a doc that ends as several do.
    doc_with: Option<syn::Path>,
    /// `doc = expr`: the doc, when it is put together from parts and cannot be `///` lines.
    doc: Option<Expr>,
    /// `constant`: a value set on a module that is not a function.
    constant: bool,
    /// `registered`: a function handed to a registration, which reads the return from the function's type.
    registered: bool,
    /// `on(m, "Duration")`: the module and the type a constant set on a type's own table belongs to.
    on: Option<(Expr, LitStr)>,
    /// Every word said, with where: what means nothing in a place is refused there, and nothing is said twice.
    words: Vec<(String, Span)>,
}

/// What can be said of a function (`lua_fn`).
const OF_A_FUNCTION: &[&str] = &[
    "name",
    "return",
    "types",
    "names",
    "takes",
    "doc",
    "doc_with",
    "registered",
];
/// What can be said of a member of a type.
const OF_A_MEMBER: &[&str] = &[
    "name", "return", "types", "names", "takes", "doc", "doc_with", "method", "function", "field",
    "skip",
];
/// What can be said of a registration given a closure (a return is refused with its own reason).
const OF_A_REGISTRATION: &[&str] = &["return", "types", "names", "takes", "doc", "doc_with"];
/// What can be said of a function set on a module.
const OF_A_SET: &[&str] = &["return", "types", "names", "takes", "doc", "doc_with"];
/// What can be said of a constant.
const OF_A_CONSTANT: &[&str] = &["return", "constant", "doc", "doc_with", "on"];

impl Said {
    /// Notes a word, and refuses one said before.
    fn word(&mut self, word: &str, span: Span) -> syn::Result<()> {
        // a kind is one thing however it is said, and so is a return
        let word = match word {
            "returns" => "return",
            "function" | "field" => "method",
            other => other,
        };
        if self.words.iter().any(|(said, _)| said == word) {
            return Err(syn::Error::new(span, "this is said twice"));
        }
        self.words.push((word.to_owned(), span));
        Ok(())
    }

    /// Refuses what means nothing in a place, so that it is not passed over in silence.
    fn only(&self, place: &str, allowed: &[&str]) -> syn::Result<()> {
        let allowed = |word: &str| {
            allowed.contains(&word) || (word == "method" && allowed.contains(&"field"))
        };
        match self.words.iter().find(|(word, _)| !allowed(word)) {
            Some((word, span)) => {
                let word = if word == "method" {
                    "a kind"
                } else {
                    word.as_str()
                };
                Err(syn::Error::new(
                    *span,
                    format!("`{word}` means nothing on {place}"),
                ))
            }
            None => Ok(()),
        }
    }

    fn read(&mut self, input: syn::parse::ParseStream) -> syn::Result<()> {
        while !input.is_empty() {
            if input.peek(LitStr) {
                let text: LitStr = input.parse()?;
                self.word("return", text.span())?;
                self.returns = Some(syn::parse_quote!(#text));
            } else {
                let word: Ident = input.parse()?;
                if !matches!(word.to_string().as_str(), "types" | "names") {
                    self.word(&word.to_string(), word.span())?;
                }
                match word.to_string().as_str() {
                    "on" => {
                        let inner;
                        syn::parenthesized!(inner in input);
                        let module = inner.parse()?;
                        inner.parse::<Token![,]>()?;
                        self.on = Some((module, inner.parse()?));
                    }
                    "skip" => self.skip = true,
                    "method" => self.kind = Some(Member::Method),
                    "function" => self.kind = Some(Member::Function),
                    "field" => self.kind = Some(Member::Field),
                    "constant" => self.constant = true,
                    "registered" => self.registered = true,
                    "name" => {
                        input.parse::<Token![=]>()?;
                        self.name = Some(input.parse()?);
                    }
                    "returns" => {
                        input.parse::<Token![=]>()?;
                        self.returns = Some(input.parse()?);
                    }
                    "doc" => {
                        input.parse::<Token![=]>()?;
                        self.doc = Some(input.parse()?);
                    }
                    "doc_with" => {
                        input.parse::<Token![=]>()?;
                        self.doc_with = Some(input.parse()?);
                    }
                    "types" | "names" => {
                        let inner;
                        syn::parenthesized!(inner in input);
                        if !self.words.iter().any(|(said, _)| word == said) {
                            self.words.push((word.to_string(), word.span()));
                        }
                        while !inner.is_empty() {
                            let binding: Ident = inner.parse()?;
                            inner.parse::<Token![=]>()?;
                            let before = if word == "types" {
                                self.types.iter().any(|(of, _)| *of == binding)
                            } else {
                                self.names.iter().any(|(of, _)| *of == binding)
                            };
                            if before {
                                return Err(syn::Error::new(binding.span(), "this is said twice"));
                            }
                            if word == "types" {
                                self.types.push((binding, inner.parse()?));
                            } else {
                                self.names.push((binding, inner.parse()?));
                            }
                            if !inner.is_empty() {
                                inner.parse::<Token![,]>()?;
                            }
                        }
                    }
                    "takes" => {
                        let inner;
                        syn::parenthesized!(inner in input);
                        let list = inner
                            .parse_terminated(<Taken as syn::parse::Parse>::parse, Token![,])?;
                        self.takes = Some(list.into_iter().collect());
                    }
                    _ => {
                        return Err(syn::Error::new(
                            word.span(),
                            "expected a return type as a string, `name = ...`, `returns = ...`, `types(...)`, `names(...)`, \
                             `takes(...)`, `doc = ...`, `doc_with = ...`, `method`, `function`, `field`, `constant`, \
                             `on(...)`, `registered` or `skip`",
                        ));
                    }
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        if let (true, Some((_, span))) = (self.skip, self.words.iter().find(|(w, _)| w != "skip")) {
            return Err(syn::Error::new(
                *span,
                "this is said of a member `skip` leaves out",
            ));
        }
        Ok(())
    }

    /// Reads the `#[lua(...)]` attributes of `attrs` and takes them out (they mean nothing to the compiler).
    fn above(attrs: &mut Vec<Attribute>) -> syn::Result<Self> {
        let mut said = Self::default();
        if let Some(second) = attrs.iter().filter(|a| a.path().is_ident("lua")).nth(1) {
            return Err(syn::Error::new(
                second.span(),
                "one `#[lua(...)]` says all there is to say of this",
            ));
        }
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
            self.no_renames()?;
            return takes
                .iter()
                .map(|taken| {
                    let pair = match taken {
                        Taken::Text(pair) => pair,
                        Taken::Named(name, ty) => {
                            let name = name.to_string();
                            return Ok(quote!((::std::string::String::from(#name), ::std::string::String::from(#ty))));
                        }
                    };
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
        let mut seen = Vec::new();
        for (pat, ty) in inputs {
            self.param(pat, ty, &mut found, &mut seen)?;
        }
        // a name that is no parameter's is a slip, not something to pass over
        match self.renamed().find(|of| !seen.contains(of)) {
            Some(of) => Err(syn::Error::new(
                of.span(),
                format!("no parameter is named `{of}`"),
            )),
            None => Ok(found),
        }
    }

    /// The parameters that `types(...)` and `names(...)` speak of.
    fn renamed(&self) -> impl Iterator<Item = &Ident> {
        self.types
            .iter()
            .map(|(of, _)| of)
            .chain(self.names.iter().map(|(of, _)| of))
    }

    /// The doc as the declarations keep it: the `///` lines or `doc = ...`, given to `doc_with` when one is said.
    fn doc(&self, lines: &str) -> syn::Result<Tokens> {
        if let (Some(doc), false) = (&self.doc, lines.is_empty()) {
            return Err(syn::Error::new(
                doc.span(),
                "the doc is said twice: here and in the `///` lines",
            ));
        }
        if self.doc.is_none() && self.doc_with.is_none() {
            return Ok(quote!(#lines));
        }
        let doc = self
            .doc
            .as_ref()
            .map_or_else(|| quote!(#lines), ToTokens::to_token_stream);
        Ok(self.doc_with.as_ref().map_or_else(
            || quote!(&*::std::string::String::from(#doc)),
            |with| quote!(&*::std::string::String::from(#with(#doc))),
        ))
    }

    /// `types(...)` and `names(...)` are for parameters read from the Rust: they mean nothing beside a list
    /// said whole, or where there is no list.
    fn no_renames(&self) -> syn::Result<()> {
        match self.renamed().next() {
            Some(of) => Err(syn::Error::new(
                of.span(),
                "`types(...)` and `names(...)` are for parameters read from the Rust: here the list is said elsewhere",
            )),
            None => Ok(()),
        }
    }

    /// What one Rust parameter is to a script: nothing for `_: ()`, one parameter for `name: T`, one each for
    /// `(a, b): (A, B)`, and any number (`...`) for `rest: Variadic<T>`.
    fn param(
        &self,
        pat: &Pat,
        ty: Option<&Type>,
        found: &mut Vec<Tokens>,
        seen: &mut Vec<Ident>,
    ) -> syn::Result<()> {
        match pat {
            Pat::Type(typed) => return self.param(&typed.pat, Some(&typed.ty), found, seen),
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
                    self.param(part, ty, found, seen)?;
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
        seen.push(binding.clone());
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

/// What a function's `Result` gives a script.
enum Returned<'t> {
    /// `mlua::Result<T>`: `T`, the error being raised.
    Value(&'t Type),
    /// `Result<T, E>`: the result itself.
    Either,
}

fn result_of(ty: &Type) -> Option<Returned<'_>> {
    let value = generic_of(ty, "Result")?;
    let Type::Path(path) = ty else { return None };
    let PathArguments::AngleBracketed(args) = &path.path.segments.last()?.arguments else {
        return None;
    };
    Some(if args.args.len() == 1 {
        Returned::Value(value)
    } else {
        Returned::Either
    })
}

/// The recorded return of a function: what was said, or the `DeclType` of `T` in `mlua::Result<T>`.
fn returns_of(said: Option<&Expr>, output: &ReturnType) -> syn::Result<Tokens> {
    if let Some(text) = said {
        return Ok(quote!(::std::string::String::from(#text)));
    }
    let facade = facade();
    match output {
        ReturnType::Default => Ok(quote!(::std::string::String::from("()"))),
        ReturnType::Type(_, ty) => match result_of(ty) {
            Some(Returned::Value(value)) => Ok(quote!(<#value as #facade::DeclType>::decl_type())),
            // a `Result<T, E>` of its own is what a script gets: a value or an error, not `T`
            Some(Returned::Either) => Ok(quote!(<#ty as #facade::DeclType>::decl_type())),
            None => Err(syn::Error::new(
                ty.span(),
                "the return is not `mlua::Result<T>`: say what a script gets with `returns = \"...\"`",
            )),
        },
    }
}

fn lua_fn_impl(args: Tokens, mut function: ItemFn) -> syn::Result<Tokens> {
    let mut said = Said::default();
    syn::parse::Parser::parse2(|input: syn::parse::ParseStream| said.read(input), args)?;
    said.only("a function", OF_A_FUNCTION)?;

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
    let returns = if said.registered {
        if let Some(returns) = &said.returns {
            return Err(syn::Error::new(
                returns.span(),
                "the return of a registered function is the registration's",
            ));
        }
        quote!(::std::string::String::new())
    } else {
        returns_of(said.returns.as_ref(), &function.sig.output)?
    };
    let doc = said.doc(&doc)?;
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
/// - The return is the `T` of `mlua::Result<T>`, or `returns = "..."`. A function handed to a registration
///   (`t.constructor("new", make)`) says `registered`: the registration reads the return itself.
/// - The doc is the `///` lines: they are the script's, and are not kept as the Rust function's.
#[proc_macro_attribute]
pub fn lua_fn(args: TokenStream, item: TokenStream) -> TokenStream {
    let function = syn::parse_macro_input!(item as ItemFn);
    lua_fn_impl(args.into(), function)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// How a member of a userdata type is added, by the name of what adds it.
#[derive(Clone, Copy, PartialEq)]
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
    // the attributes of `call(...)?;` are on the `?`: they are read from the call
    if let Expr::Try(tried) = expr {
        let above = std::mem::take(&mut tried.attrs);
        match &mut *tried.expr {
            Expr::MethodCall(call) => call.attrs.extend(above),
            Expr::Call(call) => call.attrs.extend(above),
            _ => tried.attrs = above,
        }
    }
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

/// The attributes of a statement, whatever it is: a call, a loop, a macro.
fn attrs_of(stmt: &mut Stmt) -> Option<&mut Vec<Attribute>> {
    match stmt {
        Stmt::Macro(invoked) => Some(&mut invoked.attrs),
        Stmt::Expr(expr, _) => match expr {
            Expr::Call(e) => Some(&mut e.attrs),
            Expr::MethodCall(e) => Some(&mut e.attrs),
            Expr::Try(e) => match &mut *e.expr {
                Expr::Call(inner) if e.attrs.is_empty() => Some(&mut inner.attrs),
                Expr::MethodCall(inner) if e.attrs.is_empty() => Some(&mut inner.attrs),
                _ => Some(&mut e.attrs),
            },
            Expr::ForLoop(e) => Some(&mut e.attrs),
            Expr::Macro(e) => Some(&mut e.attrs),
            Expr::If(e) => Some(&mut e.attrs),
            Expr::Block(e) => Some(&mut e.attrs),
            _ => None,
        },
        _ => None,
    }
}

/// One member as a statement on `spec`.
fn record(
    span: Span,
    kind: Member,
    name: &LitStr,
    said: &Said,
    doc: &str,
    params: Option<Vec<Tokens>>,
) -> syn::Result<Tokens> {
    said.only("a member of a type", OF_A_MEMBER)?;
    let doc = said.doc(doc)?;
    let Some(ty) = &said.returns else {
        return Err(syn::Error::new(
            span,
            "say what a script gets from this member: `#[lua(\"...\")]` above it (or `#[lua(skip)]`)",
        ));
    };
    if kind == Member::Field {
        // a field takes nothing: a word about parameters is a slip
        said.no_renames()?;
        if said.takes.is_some() {
            return Err(syn::Error::new(span, "a field takes no parameters"));
        }
        return Ok(quote!(spec.field(#name, &::std::string::String::from(#ty), #doc);));
    }
    let Some(params) = params else {
        return Err(syn::Error::new(
            span,
            "no closure to read the parameters from: say them with `takes(...)`",
        ));
    };
    let record = if kind == Member::Function {
        quote!(function)
    } else {
        quote!(method)
    };
    Ok(quote! {{
        let params: ::std::vec::Vec<(::std::string::String, ::std::string::String)> = ::std::vec![#(#params),*];
        let params: ::std::vec::Vec<(&str, &str)> = params.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
        spec.#record(#name, &params, &::std::string::String::from(#ty), #doc);
    }})
}

/// The members a statement adds where the macro cannot see how (through a helper, in a loop, by a macro of the
/// crate's own): one for each `#[lua(...)]` above it, each with its name and its parameters said, and the
/// `///` lines above each as its doc.
fn said_members(stmt: &mut Stmt) -> syn::Result<Vec<Tokens>> {
    let span = stmt.span();
    let Some(attrs) = attrs_of(stmt) else {
        return Ok(Vec::new());
    };
    if !attrs.iter().any(|a| a.path().is_ident("lua")) {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    let mut group = Vec::new();
    for attr in std::mem::take(attrs) {
        if attr.path().is_ident("doc") {
            group.push(attr);
        } else if attr.path().is_ident("lua") {
            let doc = doc_of(&group);
            group.push(attr);
            let said = Said::above(&mut group)?;
            group.clear();
            if said.skip {
                continue;
            }
            let Some(name) = &said.name else {
                return Err(syn::Error::new(
                    span,
                    "what this statement adds cannot be read from it: say the member with `name = \"...\"` and \
                     `takes(...)` (or `#[lua(skip)]`)",
                ));
            };
            let params = if said.takes.is_some() {
                Some(said.params(std::iter::empty())?)
            } else {
                None
            };
            records.push(record(
                span,
                said.kind.unwrap_or(Member::Method),
                name,
                &said,
                &doc,
                params,
            )?);
        } else {
            attrs.push(attr);
        }
    }
    if let Some(doc) = group.first() {
        return Err(syn::Error::new(
            doc.span(),
            "a doc goes above the `#[lua(...)]` it is the doc of",
        ));
    }
    Ok(records)
}

/// The record of one statement of `add_fields` or `add_methods`, as statements on `spec`: nothing for a
/// statement that adds no member a script names.
fn member(stmt: &mut Stmt) -> syn::Result<Vec<Tokens>> {
    let span = stmt.span();
    let adds = call_of(stmt).and_then(|(_, adder, _)| member_kind(&adder));
    let Some(added) = adds else {
        return said_members(stmt);
    };
    let Some((attrs, _, args)) = call_of(stmt) else {
        return Ok(Vec::new());
    };
    let doc = doc_of(attrs);
    attrs.retain(|a| !a.path().is_ident("doc"));
    let said = Said::above(attrs)?;
    let mut name = None;
    let mut closure: Option<&ExprClosure> = None;
    for arg in args {
        match arg {
            Expr::Lit(lit) => {
                if let (None, Lit::Str(text)) = (&name, &lit.lit) {
                    name = Some(text.clone());
                }
            }
            Expr::Closure(found) => {
                closure.get_or_insert(found);
            }
            _ => {}
        }
    }
    said.only("a member of a type", OF_A_MEMBER)?;
    if said.skip {
        return Ok(Vec::new());
    }
    let Some(name) = said.name.clone().or(name) else {
        return Err(syn::Error::new(
            span,
            "the name of this member is not a string here: say it with `#[lua(name = \"...\")]` (or `#[lua(skip)]`)",
        ));
    };
    let kind = said.kind.unwrap_or(added);
    // the closure's own parameters: `lua`, then `this` for a method, then what a script passes. A member that
    // is to a script what its adder does not make it (a function that takes the value itself) says its list.
    let own = if added == Member::Method { 2 } else { 1 };
    let readable = closure.filter(|_| kind == added || said.takes.is_some());
    let params = if kind == Member::Field {
        None
    } else if readable.is_some() || said.takes.is_some() {
        let inputs = readable
            .into_iter()
            .flat_map(|c| c.inputs.iter().skip(own))
            .map(|pat| (pat, None));
        Some(said.params(inputs)?)
    } else {
        None
    };
    Ok(vec![record(span, kind, &name, &said, &doc, params)?])
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
/// - A statement that adds members where this cannot see how (a helper, a loop, a macro of the crate's own)
///   says each one above it: `#[lua("ret", name = "read", takes("max: number?"))]`, once per member, a
///   method unless it says `function` or `field`. A member that is a method to a script but is added as a
///   function (it takes the value itself) says `method` and its list.
/// - Only the statements at the top of `add_fields` and `add_methods` are read.
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

fn lua_members_impl(mut function: ItemFn) -> syn::Result<Tokens> {
    let mut records = Vec::new();
    for stmt in &mut function.block.stmts {
        records.extend(member(stmt)?);
    }
    let name = quote::format_ident!("{}_members", function.sig.ident);
    let facade = facade();
    Ok(quote! {
        #function

        /// What the declarations say of the members the function of this name adds: read from it by
        /// `lua_members`.
        pub(crate) fn #name(spec: &mut impl #facade::Members) {
            #(#records)*
        }
    })
}

/// Records what the declarations say of the members a function adds to a table it builds: a type that is a
/// plain table of functions and has no `impl UserData`.
///
/// It reads the statements at the top of the function as `lua_userdata` reads those of `add_methods`, and
/// writes `<name>_members(spec)` beside it: hand that to the module (`m.userdata("Matcher", make_members)`).
///
/// - A statement that is a call is a member when the name of what is called has
///   `function`, `method` or `field` in it (and not `meta`), and that word is its kind: name the helper that
///   adds a function to the table accordingly (`matcher_function(lua, &t, "to_be", |...| ...)`).
/// - The member's name is the first string literal among the call's arguments.
/// - Its parameters are the closure's after its first (the receiver or `lua`), and `#[lua(...)]` above
///   the statement says the rest, as for `lua_userdata`.
/// - It reads only the members: where the function that builds the table is itself one a script calls,
///   `#[lua_fn]` goes on it too, below this.
#[proc_macro_attribute]
pub fn lua_members(args: TokenStream, item: TokenStream) -> TokenStream {
    if !args.is_empty() {
        return syn::Error::new(Span::call_site(), "`lua_members` takes no arguments")
            .into_compile_error()
            .into();
    }
    let function = syn::parse_macro_input!(item as ItemFn);
    lua_members_impl(function)
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
            if let Expr::MethodCall(call) = &mut *tried.expr {
                call.attrs.extend(std::mem::take(&mut tried.attrs));
            }
            &mut *tried.expr
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
    name: Expr,
    said: Said,
    doc: Tokens,
    /// Whether there are `///` lines above the statement.
    documented: bool,
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
        let marked =
            attrs_of(stmt).is_some_and(|attrs| attrs.iter().any(|a| a.path().is_ident("lua")));
        let Some(call) = call_in(stmt) else {
            if marked {
                return Err(syn::Error::new(
                    span,
                    "`#[lua]` is for a call of a method on the module: a `set` or a registration",
                ));
            }
            return Ok(None);
        };
        if !call.attrs.iter().any(|a| a.path().is_ident("lua")) {
            return Ok(None);
        }
        // what is written after the statement names the receiver again: it is a name, not a call to make twice
        if !matches!(&*call.receiver, Expr::Path(_)) {
            return Err(syn::Error::new(
                call.receiver.span(),
                "`#[lua]` is for a call on the module by its name (`m.set(...)`), not on a chain",
            ));
        }
        let doc = doc_of(&call.attrs);
        call.attrs.retain(|a| !a.path().is_ident("doc"));
        let said = Said::above(&mut call.attrs)?;
        let Some(name) = call.args.first() else {
            return Err(syn::Error::new(span, "the first argument is the name"));
        };
        let documented = !doc.is_empty();
        let doc = said.doc(&doc)?;
        let closure = call.args.iter().skip(1).find_map(closure_in).cloned();
        let path = match call.args.iter().nth(1) {
            Some(Expr::Path(path)) if said.takes.is_none() => Some(path.clone()),
            _ => None,
        };
        Ok(Some(Self {
            name: name.clone(),
            said,
            doc,
            documented,
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
fn written(stmt: &mut Stmt) -> syn::Result<Option<Tokens>> {
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
        if marked.said.constant {
            marked.said.only("a constant", OF_A_CONSTANT)?;
            // a constant on a type's own table is the type's: the module is told which
            if let Some((module, of)) = &marked.said.on {
                return Ok(Some(quote! {
                    #stmt
                    #module.type_const(#of, #name, &::std::string::String::from(#returns), #doc);
                }));
            }
            return Ok(Some(quote! {
                #stmt
                #receiver.const_(#name, &::std::string::String::from(#returns), #doc);
            }));
        }
        marked.said.only("a function set on a module", OF_A_SET)?;
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
    if marked.said.constant {
        return Err(syn::Error::new(span, "`constant` is for a `set`"));
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
        marked.said.only(
            "a registration given a function by its path: the function says it all",
            &[],
        )?;
        if marked.documented {
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
    marked.said.only("a registration", OF_A_REGISTRATION)?;
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

/// Rewrites the marked statements of every block of a function: its own, and those of the loops, the
/// branches and the closures in it (a type's constructors are registered in the closure given to `type_`).
struct Registrations(Option<syn::Error>);

impl syn::visit_mut::VisitMut for Registrations {
    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        let mut out = Vec::with_capacity(block.stmts.len());
        for mut stmt in std::mem::take(&mut block.stmts) {
            let written = written(&mut stmt).and_then(|tokens| {
                tokens
                    .map(|t| syn::parse2::<syn::Block>(quote!({ #t })))
                    .transpose()
            });
            match written {
                Ok(Some(wrapped)) => out.extend(wrapped.stmts),
                Ok(None) => {
                    syn::visit_mut::visit_stmt_mut(self, &mut stmt);
                    out.push(stmt);
                }
                Err(error) => {
                    self.0.get_or_insert(error);
                    out.push(stmt);
                }
            }
        }
        block.stmts = out;
    }
}

fn lua_module_impl(mut function: ItemFn) -> syn::Result<Tokens> {
    let mut registrations = Registrations(None);
    syn::visit_mut::VisitMut::visit_block_mut(&mut registrations, &mut function.block);
    match registrations.0 {
        Some(error) => Err(error),
        None => Ok(function.to_token_stream()),
    }
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
///   `#[lua("...")]` says;
/// - `m.set("name", <a value>)?` under `#[lua("type", constant)]`: a constant of that type.
///   `on(m, "Type")` beside it says the value is set on the sub-table of the type `Type` of the module `m`
///   (`#[lua("Duration", constant, on(m, "Duration"))] dur.set("ZERO", ...)?`).
///
/// A bare `#[lua]` is enough for a registration given a function by its path, which says everything itself.
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

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::parse_quote;

    use super::{lua_fn_impl, lua_members_impl, lua_module_impl, lua_userdata_impl};

    /// What the compiler says of `result` when it is refused.
    fn refusal(result: syn::Result<proc_macro2::TokenStream>) -> String {
        match result {
            Ok(tokens) => format!("accepted: {tokens}"),
            Err(error) => error.to_string(),
        }
    }

    #[track_caller]
    fn refuses(result: syn::Result<proc_macro2::TokenStream>, why: &str) {
        let said = refusal(result);
        assert!(said.contains(why), "expected `{why}`, got `{said}`");
    }

    #[test]
    fn a_function_is_accepted_as_it_is_written() {
        let out = lua_fn_impl(
            quote!(),
            parse_quote!(
                fn f(_: &Lua, a: String) -> mlua::Result<String> {
                    Ok(a)
                }
            ),
        );
        let text = out.map_or_default(|t| t.to_string());
        assert!(text.contains("mod f"), "{text}");
    }

    #[test]
    fn a_result_with_its_own_error_is_recorded_whole() {
        let out = lua_fn_impl(
            quote!(),
            parse_quote!(
                fn f(n: f64) -> Result<f64, String> {
                    Ok(n)
                }
            ),
        );
        let text = out.map_or_default(|t| t.to_string());
        assert!(text.contains("< Result < f64 , String > as"), "{text}");
    }

    #[test]
    fn a_member_left_out_says_nothing_else() {
        let read = |input: syn::parse::ParseStream| crate::Said::default().read(input);
        let said = syn::parse::Parser::parse2(read, quote!(skip, "number"));
        refuses(
            said.map(|()| proc_macro2::TokenStream::new()),
            "`skip` leaves out",
        );
    }

    #[test]
    fn a_function_refuses_what_it_cannot_read() {
        refuses(
            lua_fn_impl(
                quote!(skip),
                parse_quote!(
                    fn f() {}
                ),
            ),
            "`skip` means nothing",
        );
        refuses(
            lua_fn_impl(
                quote!(),
                parse_quote!(
                    fn f() -> f64 {
                        1.0
                    }
                ),
            ),
            "say what a script gets",
        );
        refuses(
            lua_fn_impl(
                quote!(),
                parse_quote!(
                    fn f((a, _): (String, String)) -> mlua::Result<()> {
                        Ok(())
                    }
                ),
            ),
            "needs a name",
        );
        refuses(
            lua_fn_impl(
                quote!(),
                parse_quote!(
                    fn f(&self) {}
                ),
            ),
            "free function",
        );
        refuses(
            lua_fn_impl(
                quote!(colour = "red"),
                parse_quote!(
                    fn f() {}
                ),
            ),
            "expected a return type",
        );
    }

    #[test]
    fn a_name_that_is_no_parameters_is_refused() {
        refuses(
            lua_fn_impl(
                quote!(names(txt = "text")),
                parse_quote!(
                    fn f(text: String) -> mlua::Result<()> {
                        Ok(())
                    }
                ),
            ),
            "no parameter is named `txt`",
        );
        refuses(
            lua_fn_impl(
                quote!(types(nope = "string")),
                parse_quote!(
                    fn f(text: String) -> mlua::Result<()> {
                        Ok(())
                    }
                ),
            ),
            "no parameter is named `nope`",
        );
        refuses(
            lua_fn_impl(
                quote!(takes("a: string"), types(text = "string")),
                parse_quote!(
                    fn f(text: String) -> mlua::Result<()> {
                        Ok(())
                    }
                ),
            ),
            "the list is said elsewhere",
        );
    }

    #[test]
    fn a_member_refuses_what_it_cannot_read() {
        let block = |body: proc_macro2::TokenStream| -> syn::ItemImpl {
            parse_quote!(impl UserData for X { fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) { #body } })
        };
        refuses(
            lua_userdata_impl(block(
                quote!(methods.add_method("x", |_, this, ()| Ok(()));),
            )),
            "say what a script gets",
        );
        refuses(
            lua_userdata_impl(block(
                quote!(#[lua("number")] methods.add_method("x", Self::helper);),
            )),
            "no closure to read the parameters from",
        );
        refuses(
            lua_userdata_impl(block(
                quote!(#[lua("number")] methods.add_method(NAME, |_, this, ()| Ok(1));),
            )),
            "the name of this member is not a string",
        );
        refuses(
            lua_userdata_impl(block(quote!(#[lua("number")] methods.document("x");))),
            "what this statement adds cannot be read from it",
        );
        refuses(
            lua_userdata_impl(block(
                quote!(#[lua("number", names(b = "by"))] methods.add_method("x", |_, this, a: f64| Ok(a));),
            )),
            "no parameter is named `b`",
        );
    }

    #[test]
    fn a_registration_refuses_what_means_nothing_there() {
        let declare = |body: proc_macro2::TokenStream| -> syn::ItemFn {
            parse_quote!(fn declare() { #body })
        };
        refuses(
            lua_module_impl(declare(
                quote!(#[lua(name = "other")] m.func("upper", |_, t: String| Ok(t))?;),
            )),
            "`name` means nothing",
        );
        refuses(
            lua_module_impl(declare(
                quote!(#[lua(skip)] m.func("upper", |_, t: String| Ok(t))?;),
            )),
            "`skip` means nothing",
        );
        refuses(
            lua_module_impl(declare(
                quote!(#[lua("string")] m.func("upper", |_, t: String| Ok(t))?;),
            )),
            "it is not said by hand",
        );
        refuses(
            lua_module_impl(declare(
                quote!(#[lua(names(t = "text"))] m.try_func("parse", parse)?;),
            )),
            "`names` means nothing",
        );
        refuses(
            lua_module_impl(declare(quote!(
                /// A doc.
                #[lua]
                m.try_func("parse", parse)?;
            ))),
            "the doc of a function given by its path",
        );
        refuses(
            lua_module_impl(declare(quote!(#[lua("number")] m.set("now", now_fn)?;))),
            "no closure to read the parameters from",
        );
        refuses(
            lua_module_impl(declare(quote!(#[lua] m.set("one", make(|_, ()| Ok(1)))?;))),
            "say what a script gets",
        );
        refuses(
            lua_module_impl(declare(quote!(#[lua] m.other("x", |_, ()| Ok(1))?;))),
            "is for a `set` or a registration",
        );
    }

    #[test]
    fn a_constant_is_a_set_with_a_type_and_nothing_else() {
        let declare = |body: proc_macro2::TokenStream| -> syn::ItemFn {
            parse_quote!(fn declare() { #body })
        };
        let out = lua_module_impl(declare(quote!(
            /// The system.
            #[lua("string", constant)]
            m.set("os", OS)?;
        )));
        let text = out.map_or_default(|t| t.to_string());
        assert!(text.contains("m . const_ (\"os\""), "{text}");
        assert!(text.contains("\"The system.\""), "{text}");
        refuses(
            lua_module_impl(declare(quote!(#[lua(constant)] m.set("os", OS)?;))),
            "say what a script gets",
        );
        refuses(
            lua_module_impl(declare(
                quote!(#[lua(constant)] m.func("os", |_, ()| Ok(1))?;),
            )),
            "`constant` is for a `set`",
        );
        refuses(
            lua_module_impl(declare(
                quote!(#[lua("string", constant, takes("a: string"))] m.set("os", OS)?;),
            )),
            "`takes` means nothing on a constant",
        );
        refuses(
            lua_fn_impl(
                quote!(constant),
                parse_quote!(
                    fn f() {}
                ),
            ),
            "`constant` means nothing",
        );
    }

    #[test]
    fn a_name_a_type_and_a_doc_can_be_computed() {
        let declare = |body: proc_macro2::TokenStream| -> syn::ItemFn {
            parse_quote!(fn declare() { #body })
        };
        let out = lua_module_impl(declare(quote!(for name in NAMES {
            /// Waits.
            #[lua(returns = RESULT, types(opts = &format!("{OPTIONS}?")), doc_with = waits)]
            m.set(name, make(|_, opts: Table| Ok(1)))?;
        })));
        let text = out.map_or_default(|t| t.to_string());
        assert!(text.contains("from (name)"), "{text}");
        assert!(text.contains("from (RESULT)"), "{text}");
        assert!(text.contains("format ! (\"{OPTIONS}?\")"), "{text}");
        assert!(text.contains("from (waits (\"Waits.\"))"), "{text}");
    }

    #[test]
    fn a_statement_that_adds_members_out_of_sight_says_each_one() {
        let block = |body: proc_macro2::TokenStream| -> syn::ItemImpl {
            parse_quote!(impl UserData for X { fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) { #body } })
        };
        let out = lua_userdata_impl(block(quote!(
            /// Read from the output.
            #[lua("string?", name = "read", takes("max: number?"))]
            /// Read from the errors.
            #[lua(returns = RESULT, name = "read_stderr", takes(opts = OPTIONS), function)]
            for source in &SOURCES {
                add_read(methods, source);
            }
        )));
        let text = out.map_or_default(|t| t.to_string());
        assert!(text.contains("spec . method (\"read\""), "{text}");
        assert!(text.contains("\"Read from the output.\""), "{text}");
        assert!(text.contains("spec . function (\"read_stderr\""), "{text}");
        assert!(text.contains("\"Read from the errors.\""), "{text}");
        assert!(
            text.contains("from (\"opts\") , :: std :: string :: String :: from (OPTIONS)"),
            "{text}"
        );
        assert!(!text.contains("# [lua"), "{text}");
        refuses(
            lua_userdata_impl(block(
                quote!(#[lua("number", name = "x")] helper!(methods, "x");),
            )),
            "no closure to read the parameters from",
        );
    }

    #[test]
    fn a_function_can_be_a_method_to_a_script() {
        let block = |body: proc_macro2::TokenStream| -> syn::ItemImpl {
            parse_quote!(impl UserData for X { fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) { #body } })
        };
        let out = lua_userdata_impl(block(quote!(
            #[lua("Duration", method, takes("other: Instant"))]
            methods.add_function("since", |_, args: (AnyUserData, AnyUserData)| Ok(1));
        )));
        let text = out.map_or_default(|t| t.to_string());
        assert!(text.contains("spec . method (\"since\""), "{text}");
        refuses(
            lua_userdata_impl(block(quote!(
                #[lua("Duration", method)]
                methods.add_function("since", |_, args: (AnyUserData, AnyUserData)| Ok(1));
            ))),
            "no closure to read the parameters from",
        );
    }

    #[test]
    fn a_registered_function_leaves_its_return_to_the_registration() {
        let out = lua_fn_impl(
            quote!(registered),
            parse_quote!(
                fn make(_: &Lua, _: ()) -> mlua::Result<Map> {
                    todo!()
                }
            ),
        );
        let text = out.map_or_default(|t| t.to_string());
        assert!(text.contains("String :: new ()"), "{text}");
        assert!(!text.contains("DeclType"), "{text}");
        refuses(
            lua_fn_impl(
                quote!(registered, returns = "Map"),
                parse_quote!(
                    fn make() -> mlua::Result<Map> {
                        todo!()
                    }
                ),
            ),
            "is the registration's",
        );
    }

    #[test]
    fn a_function_that_builds_a_table_declares_its_members() {
        let out = lua_members_impl(parse_quote!(
            fn make(lua: &Lua, value: Value) -> mlua::Result<Table> {
                let m = lua.create_table()?;
                /// Assert the value equals `expected`.
                #[lua("()")]
                matcher_function(lua, &m, "toBe", value.clone(), |actual, expected: Value| {
                    Ok(())
                })?;
                Ok(m)
            }
        ));
        let text = out.map_or_default(|t| t.to_string());
        assert!(text.contains("fn make_members"), "{text}");
        assert!(text.contains("spec . function (\"toBe\""), "{text}");
        assert!(text.contains("from (\"expected\")"), "{text}");
    }

    #[allow(clippy::needless_pass_by_value)]
    fn in_methods(body: proc_macro2::TokenStream) -> syn::ItemImpl {
        parse_quote!(impl UserData for X { fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) { #body } })
    }

    #[allow(clippy::needless_pass_by_value)]
    fn in_declare(body: proc_macro2::TokenStream) -> syn::ItemFn {
        parse_quote!(fn declare() { #body })
    }

    #[test]
    fn a_word_that_means_nothing_where_it_is_written_is_refused() {
        refuses(
            lua_fn_impl(
                quote!(field),
                parse_quote!(
                    fn f() {}
                ),
            ),
            "`a kind` means nothing on a function",
        );
        refuses(
            lua_fn_impl(
                quote!(on(m, "T")),
                parse_quote!(
                    fn f() {}
                ),
            ),
            "`on` means nothing on a function",
        );
        refuses(
            lua_userdata_impl(in_methods(
                quote!(#[lua("number", registered)] methods.add_method("x", |_, this, ()| Ok(1));),
            )),
            "`registered` means nothing on a member",
        );
        refuses(
            lua_userdata_impl(in_methods(
                quote!(#[lua("number", constant)] methods.add_method("x", |_, this, ()| Ok(1));),
            )),
            "`constant` means nothing on a member",
        );
        refuses(
            lua_userdata_impl(in_methods(
                quote!(#[lua("number", name = "x", registered, takes())] helper(methods);),
            )),
            "`registered` means nothing on a member",
        );
        refuses(
            lua_module_impl(in_declare(
                quote!(#[lua("x", registered)] m.set("a", make(|_, ()| Ok(1)))?;),
            )),
            "`registered` means nothing on a function set on a module",
        );
        refuses(
            lua_module_impl(in_declare(
                quote!(#[lua("x", method)] m.set("a", make(|_, ()| Ok(1)))?;),
            )),
            "`a kind` means nothing on a function set on a module",
        );
        refuses(
            lua_module_impl(in_declare(
                quote!(#[lua(function)] m.func("a", |_, ()| Ok(1))?;),
            )),
            "`a kind` means nothing on a registration",
        );
        refuses(
            lua_module_impl(in_declare(
                quote!(#[lua(doc = "x")] m.try_func("parse", parse)?;),
            )),
            "`doc` means nothing on a registration given a function by its path",
        );
        refuses(
            lua_module_impl(in_declare(
                quote!(#[lua(doc_with = waits)] m.try_func("parse", parse)?;),
            )),
            "`doc_with` means nothing on a registration given a function by its path",
        );
    }

    #[test]
    fn nothing_is_said_twice() {
        refuses(
            lua_fn_impl(
                quote!("a", returns = "b"),
                parse_quote!(
                    fn f() {}
                ),
            ),
            "this is said twice",
        );
        refuses(
            lua_fn_impl(
                quote!(name = "a", name = "b"),
                parse_quote!(
                    fn f() {}
                ),
            ),
            "this is said twice",
        );
        refuses(
            lua_fn_impl(
                quote!(types(a = "x"), types(a = "y")),
                parse_quote!(
                    fn f(a: String) {}
                ),
            ),
            "this is said twice",
        );
        refuses(
            lua_userdata_impl(in_methods(
                quote!(#[lua("number", method, function, takes())] methods.add_function("x", |_, ()| Ok(1));),
            )),
            "this is said twice",
        );
        refuses(
            lua_userdata_impl(in_methods(quote!(
                #[lua("number")]
                #[lua(names(a = "b"))]
                methods.add_method("x", |_, this, a: f64| Ok(a));
            ))),
            "one `#[lua(...)]` says all there is to say",
        );
        refuses(
            lua_module_impl(in_declare(quote!(
                /// A doc.
                #[lua("number", doc = "Another.")]
                m.set("a", make(|_, ()| Ok(1)))?;
            ))),
            "the doc is said twice",
        );
    }

    #[test]
    fn a_field_takes_nothing() {
        let in_fields = |body: proc_macro2::TokenStream| -> syn::ItemImpl {
            parse_quote!(impl UserData for X { fn add_fields<F: UserDataFields<Self>>(fields: &mut F) { #body } })
        };
        refuses(
            lua_userdata_impl(in_fields(
                quote!(#[lua("number", takes("a: string"))] fields.add_field_method_get("x", |_, this| Ok(1));),
            )),
            "a field takes no parameters",
        );
        refuses(
            lua_userdata_impl(in_fields(
                quote!(#[lua("number", types(a = "string"))] fields.add_field_method_get("x", |_, this| Ok(1));),
            )),
            "the list is said elsewhere",
        );
    }

    #[test]
    fn a_mark_is_never_dropped() {
        refuses(
            lua_module_impl(in_declare(quote!(#[lua("x", constant)] helper(m, 1)?;))),
            "`#[lua]` is for a call of a method on the module",
        );
        refuses(
            lua_module_impl(in_declare(quote!(
                #[lua("x")]
                for name in NAMES {
                    m.set(name, 1)?;
                }
            ))),
            "`#[lua]` is for a call of a method on the module",
        );
        refuses(
            lua_module_impl(in_declare(
                quote!(#[lua("x", constant)] m.set("a", 1)?.set("b", 2)?;),
            )),
            "not on a chain",
        );
        refuses(
            lua_userdata_impl(in_methods(quote!(
                #[lua("number", name = "x", takes())]
                /// A doc below its mark.
                helper(methods);
            ))),
            "a doc goes above",
        );
    }

    #[test]
    fn a_constant_on_a_type_is_told_its_module_and_its_type() {
        let out = lua_module_impl(in_declare(quote!(
            /// Nothing.
            #[lua("Duration", constant, on(m, "Duration"))]
            dur.set("ZERO", zero())?;
        )));
        let text = out.map_or_default(|t| t.to_string());
        assert!(
            text.contains("m . type_const (\"Duration\" , \"ZERO\""),
            "{text}"
        );
    }
}
