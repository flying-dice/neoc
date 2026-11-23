//! The attributes neoc-vm's modules declare their Lua surface with.
//!
//! A function a script calls is implemented once, in Rust, and what the declarations say of it (its name, its
//! parameters and their types, its doc) is read from that implementation instead of being written again beside
//! it:
//!
//! ```ignore
//! /// Decode a standard-alphabet base64 string back to raw bytes.
//! #[lua_fn]
//! fn decode(lua: &Lua, input: mlua::LuaString) -> mlua::Result<Fallible<mlua::LuaString>> { ... }
//!
//! m.declared(decode::lua(), lua.create_function(decode)?)?;
//! ```
//!
//! The macros write paths into neoc-vm's facade (`crate::lua::facade`), so they are for neoc-vm's own modules.
//!
//! [`lua_fn`] is for a function, [`lua_userdata`] for the methods and fields of an `impl UserData` block
//! ([`lua_members`] for a type that is a plain table a function builds), and
//! [`lua_module`] for the closures a module's `declare` function registers in place. [`lua_record`] is for a
//! struct that is a table to a script.
//!
//! What is read: the name (the function's, or the string a method is added under), each parameter's name and
//! its type through `DeclType` (`String` is `string`, `Option<i64>` is `number?`), and the `///` lines as the doc.
//!
//! What a function returns is read from its type too, and for a closure, whose return is written nowhere, the
//! compiler is asked: the closure goes to mlua through a function of `facade::typed` whose bounds name the
//! type. So a function says what a script gets by returning it:
//!
//! | The Rust type | What a script gets |
//! | --- | --- |
//! | `Fallible<T>` (`ok(v)`, `err(msg)`) | `(T?, string?)`: the value, or `nil` and why not |
//! | `Option<T>` | `T?` |
//! | a userdata type that says its name (`#[lua_userdata("std:time", "Duration")]`) | that type |
//! | `Handle<T>`, as a parameter | a userdata declared as `T`, borrowed by the function |
//! | `List<T>` | `{ T }` |
//! | `Bytes` | `string`: bytes that need not be text |
//! | a struct that is a table (`#[lua_record(into)]`, `from`) | `{ field: T, ... }`, read from its fields |
//! | `Checked<T>`, as a parameter | `T`, read from what the script passed; a refusal is the function's to return (`opts.take()?`) |
//! | `Declared<T, V>`, as a parameter | the value `V` as it was passed, declared as `T`: for a value the function reads itself |
//! | a type of the module's own with a `DeclType` (an enum of the kinds a function can hand back) | what it says |
//!
//! A member a helper adds for its caller (`call(methods, "keyring.entry", ...)`) says its name and its
//! parameters above the statement, and the helper says what it returns from the type it is generic over
//! (`typed::answers::<R>()`).
//!
//! What Rust does not say is said once, on the spot: a return that is a `Table` or a `Value` in Rust and
//! something narrower to a script,
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
    Attribute, Expr, ExprClosure, FnArg, GenericArgument, Ident, ImplItem, ImplItemFn, ItemFn,
    ItemImpl, Lit, LitStr, Pat, PathArguments, ReturnType, Stmt, Token, Type,
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

/// `T` of `impl Future<Output = T>`: what a function that is not `async fn` but returns a future comes to.
fn output_of(ty: &Type) -> Option<&Type> {
    let Type::ImplTrait(opaque) = ty else {
        return None;
    };
    opaque.bounds.iter().find_map(|bound| {
        let syn::TypeParamBound::Trait(bound) = bound else {
            return None;
        };
        let last = bound.path.segments.last()?;
        let PathArguments::AngleBracketed(args) = &last.arguments else {
            return None;
        };
        args.args.iter().find_map(|arg| match arg {
            GenericArgument::AssocType(output) if output.ident == "Output" => Some(&output.ty),
            _ => None,
        })
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
        // a function that hands back a future says in its `Output` what the future comes to
        ReturnType::Type(_, ty) => match result_of(output_of(ty).unwrap_or(ty)) {
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
    noted: bool,
) -> syn::Result<Tokens> {
    let facade = facade();
    said.only("a member of a type", OF_A_MEMBER)?;
    let doc = said.doc(doc)?;
    let Some(ty) = &said.returns else {
        // added out of sight (by a helper, in a loop) with nothing said of what it returns: the helper says
        // it, from the type of what it registers (`typed::answers`)
        if let (true, Some(params), false) = (noted, &params, kind == Member::Field) {
            let kind = if kind == Member::Function {
                "function"
            } else {
                "method"
            };
            return Ok(quote! {
                #facade::typed::left(#kind, #name, || ::std::vec![#(#params),*], #doc);
            });
        }
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
        return Ok(if noted {
            quote!(#facade::typed::said("field", #name, ::std::vec::Vec::new, &::std::string::String::from(#ty), #doc);)
        } else {
            quote!(spec.field(#name, &::std::string::String::from(#ty), #doc);)
        });
    }
    let Some(params) = params else {
        return Err(syn::Error::new(
            span,
            "no closure to read the parameters from: say them with `takes(...)`",
        ));
    };
    if noted {
        let kind = if kind == Member::Function {
            "function"
        } else {
            "method"
        };
        return Ok(quote! {
            #facade::typed::said(#kind, #name, || ::std::vec![#(#params),*], &::std::string::String::from(#ty), #doc);
        });
    }
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
fn said_members(stmt: &mut Stmt, noted: bool) -> syn::Result<Vec<Tokens>> {
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
                noted,
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
fn member(stmt: &mut Stmt, noted: bool) -> syn::Result<Vec<Tokens>> {
    let span = stmt.span();
    let called = call_of(stmt).map(|(_, called, _)| called);
    let Some((adder, made)) = called.and_then(|a| member_kind(&a).map(|kind| (a, kind))) else {
        return said_members(stmt, noted);
    };
    let Some((attrs, _, mut args)) = call_of(stmt) else {
        return Ok(Vec::new());
    };
    let doc = doc_of(attrs);
    attrs.retain(|a| !a.path().is_ident("doc"));
    let said = Said::above(attrs)?;
    let mut name = None;
    let mut at = None;
    for (index, arg) in args.iter().enumerate() {
        match &**arg {
            Expr::Lit(lit) => {
                if let (None, Lit::Str(text)) = (&name, &lit.lit) {
                    name = Some(text.clone());
                }
            }
            Expr::Closure(_) => {
                at.get_or_insert(index);
            }
            _ => {}
        }
    }
    let closure = at.and_then(|index| match &*args[index] {
        Expr::Closure(found) => Some(found.clone()),
        _ => None,
    });
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
    let kind = said.kind.unwrap_or(made);
    // the closure's own parameters: `lua`, then `this` for a method, then what a script passes. A member that
    // is to a script what its adder does not make it (a function that takes the value itself) says its list.
    let own = if made == Member::Method { 2 } else { 1 };
    let readable = closure
        .as_ref()
        .filter(|_| kind == made || said.takes.is_some());
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
    // what the closure returns is the compiler's to say: the closure goes to mlua through a function whose
    // bounds name the type
    if let (true, None, Some(at), Some(closure)) = (
        noted && kind == made && INFERRED.contains(&adder.as_str()),
        &said.returns,
        at,
        &closure,
    ) {
        let facade = facade();
        // the crate's own `lifecycle::async_method` takes the closure `add_async_method` takes
        let through = if adder == "async_method" {
            "add_async_method"
        } else {
            adder.as_str()
        };
        let through = Ident::new(through, Span::call_site());
        // the type the closure is a member of is said, so that `this` has its type inside the closure
        let through = match adder.as_str() {
            "add_method" | "add_method_mut" => quote!(#through::<Self, _, _, _, _>),
            "add_async_method" | "add_async_method_mut" | "async_method" => {
                quote!(#through::<Self, _, _, _, _, _>)
            }
            "add_field_method_get" => quote!(#through::<Self, _, _>),
            _ => quote!(#through),
        };
        let doc = said.doc(&doc)?;
        *args[at] = if kind == Member::Field {
            said.no_renames()?;
            if said.takes.is_some() {
                return Err(syn::Error::new(span, "a field takes no parameters"));
            }
            syn::parse_quote!(#facade::typed::#through(#closure, #name, #doc))
        } else {
            let params = params.unwrap_or_default();
            syn::parse_quote!(#facade::typed::#through(
                #closure,
                #facade::typed::Of { name: #name, params: || ::std::vec![#(#params),*], doc: #doc },
            ))
        };
        return Ok(Vec::new());
    }
    Ok(vec![record(span, kind, &name, &said, &doc, params, noted)?])
}

/// The adders whose closure is passed through `facade::typed`, which reads what it returns from its type.
const INFERRED: &[&str] = &[
    "add_method",
    "add_method_mut",
    "add_async_method",
    "add_async_method_mut",
    "async_method",
    "add_function",
    "add_function_mut",
    "add_async_function",
    "add_field_method_get",
    "add_field_function_get",
];

fn lua_userdata_impl(mut block: ItemImpl, named: &Named) -> syn::Result<Tokens> {
    for item in &mut block.items {
        let ImplItem::Fn(function) = item else {
            continue;
        };
        let name = function.sig.ident.to_string();
        if name != "add_fields" && name != "add_methods" {
            continue;
        }
        // a member said by hand is noted where it is added, so that the notes keep the order of the block
        let mut stmts = Vec::new();
        for mut stmt in std::mem::take(&mut function.block.stmts) {
            for said in member(&mut stmt, true)? {
                stmts.push(syn::parse2(said)?);
            }
            stmts.push(stmt);
        }
        function.block.stmts = stmts;
    }
    let ty = &block.self_ty;
    let (generics, _, where_clause) = block.generics.split_for_impl();
    let facade = facade();
    // a type that says its name is one a function can take or return: the name is its declared type
    let name = named.0.as_ref().map(|(module, name)| {
        quote! {
            impl #generics #facade::DeclType for #ty #where_clause {
                fn decl_type() -> ::std::string::String {
                    ::std::format!("{}.{}", #facade::luau_decls::scope(#module), #name)
                }
            }

            impl #generics #facade::Named for #ty #where_clause {
                const MODULE: &'static str = #module;
                const NAME: &'static str = #name;
            }
        }
    });
    Ok(quote! {
        #block

        #name

        impl #generics #ty #where_clause {
            /// What the declarations say of this type's members: read from its `impl UserData` by
            /// `lua_userdata`.
            pub(crate) fn lua(spec: &mut impl #facade::Members) {
                #facade::typed::record::<Self>(spec);
            }
        }
    })
}

/// Records what the declarations say of a userdata type's members from its `impl UserData` block.
///
/// It writes `Type::lua(spec)`, which records every member the block adds at the top of `add_fields` and
/// `add_methods`, in order: hand it to the module (`m.userdata("Version", VersionHandle::lua)`).
///
/// - `#[lua_userdata("std:time", "Duration")]` says the module the type is declared by and its name there:
///   a function that takes or returns the type then declares it by its Rust type.
/// - The name is the string the member is added under.
/// - What a script gets (a field's type, a method's return) is what the closure returns: the closure is
///   passed to mlua through `facade::typed`, which reads the type. `#[lua("...")]` above the statement says
///   it by hand where the Rust type says less than a script should know (a `Table`, a `Value`).
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
    let named = syn::parse_macro_input!(args as Named);
    let block = syn::parse_macro_input!(item as ItemImpl);
    lua_userdata_impl(block, &named)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// What `lua_userdata` is told of the type: nothing, or the module it is a type of and its name there
/// (`"std:time", "Duration"`).
#[derive(Default)]
struct Named(Option<(LitStr, LitStr)>);

impl syn::parse::Parse for Named {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(Self(None));
        }
        let module = input.parse()?;
        input.parse::<Token![,]>()?;
        let name = input.parse()?;
        Ok(Self(Some((module, name))))
    }
}

fn lua_members_impl(mut function: ItemFn) -> syn::Result<Tokens> {
    let mut records = Vec::new();
    for stmt in &mut function.block.stmts {
        records.extend(member(stmt, false)?);
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
            return inferred_set(stmt, &marked, span);
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

/// Replaces the first closure of an expression with a call that is given it: the closure [`closure_in`] finds,
/// which the parameters are read from (both stop at the first closure in the same order).
struct Through(Option<Tokens>);

impl syn::visit_mut::VisitMut for Through {
    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        if self.0.is_none() {
            return;
        }
        if let Expr::Closure(closure) = expr {
            let sync = !waits(closure);
            if let Some(facade) = self.0.take() {
                *expr = if sync {
                    syn::parse_quote!(#facade::typed::returning(&__lua_ret, #closure))
                } else {
                    syn::parse_quote!(#facade::typed::returning_async(&__lua_ret, #closure))
                };
            }
            return;
        }
        syn::visit_mut::visit_expr_mut(self, expr);
    }
}

/// Whether a closure hands back a future: it is `async`, or what it comes to is an `async` block (with or
/// without a method called on it).
fn waits(closure: &ExprClosure) -> bool {
    fn is_async(expr: &Expr) -> bool {
        match expr {
            Expr::Async(_) => true,
            // a future a method was called on is still one (`async move { ... }.instrument(span)`)
            Expr::MethodCall(call) => is_async(&call.receiver),
            Expr::Block(block) => {
                matches!(block.block.stmts.last(), Some(Stmt::Expr(last, None)) if is_async(last))
            }
            _ => false,
        }
    }
    closure.asyncness.is_some() || is_async(&closure.body)
}

/// A `set` that does not say what a script gets: the compiler says it. A function's closure goes through
/// `typed::returning`, whose bounds name what it returns, and a constant's value through `typed::constant`.
fn inferred_set(stmt: &mut Stmt, marked: &Marked, span: Span) -> syn::Result<Option<Tokens>> {
    let facade = facade();
    let (name, doc, receiver) = (&marked.name, &marked.doc, &marked.receiver);
    if marked.said.constant {
        marked.said.only("a constant", OF_A_CONSTANT)?;
        let Some(value) = call_in(stmt).and_then(|call| call.args.iter_mut().nth(1)) else {
            return Err(syn::Error::new(span, "a constant is set to a value"));
        };
        *value = syn::parse_quote!(#facade::typed::constant(&__lua_ret, #value));
        let record = if let Some((module, of)) = &marked.said.on {
            quote!(#module.type_const(#of, #name, &__lua_ret, #doc);)
        } else {
            quote!(#receiver.const_(#name, &__lua_ret, #doc);)
        };
        return Ok(Some(quote! {
            let __lua_ret = ::std::cell::OnceCell::new();
            #stmt
            let __lua_ret = __lua_ret.into_inner().unwrap_or_default();
            #record
        }));
    }
    if marked.closure.is_none() {
        return Err(syn::Error::new(
            span,
            "say what a script gets from this function: `#[lua(\"...\")]`",
        ));
    }
    marked.said.only("a function set on a module", OF_A_SET)?;
    let params = marked.params(span)?;
    let mut through = Through(Some(facade.clone()));
    syn::visit_mut::VisitMut::visit_stmt_mut(&mut through, stmt);
    Ok(Some(quote! {
        let __lua_ret = ::std::cell::OnceCell::new();
        #stmt
        #receiver.record(#facade::FnDecl {
            name: ::std::string::String::from(#name),
            params: ::std::vec![#(#params),*],
            ret: __lua_ret.into_inner().unwrap_or_default(),
            doc: ::std::string::String::from(#doc),
        });
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

/// The methods of a userdata type, each a real method of the type: `#[lua_methods("vnd:bollard", "Client")]`
/// on an `impl Client { ... }` block. Everything the declarations say of a method is read from its signature:
///
/// ```ignore
/// /// Stop a container.
/// async fn stop_container(&self, name: String, opts: Option<Stop>) -> Result<bool, BollardError> { ... }
/// ```
///
/// is `stop_container: (self: Client, name: string, opts: { ... }?) -> (boolean?, string?)`. The name is the
/// method's, the parameters are its own, the doc is its `///` lines, and the return is its type: a
/// `Result<T, E>` is `(T?, string?)`, with the error returned as `<module>.<method>: <error>`.
///
/// Each argument is read by `facade::args::Argument` before the method is called, and for an `async` method
/// before its future exists. A table of options that is refused is returned as the method's own error
/// (`E: From<Refused>`). An `async` method is called on a clone of the value, so the type is `Clone`.
///
/// The block is the type's `impl UserData`: every method with a `self` receiver is a method a script calls,
/// unless it says `#[lua(skip)]`. `#[lua(name = "...")]` gives another name, `#[lua(role = Mixed)]` the
/// `lifecycle::Role` of an `async` method (`Wait` when not said), `#[lua(doc = EXPR)]` a doc that is put
/// together. A leading parameter of type `&Lua` (`Lua` for an `async` method) is the engine, and one of type
/// `Op` is the method's name as its errors carry it (`bollard.logs`): neither is an argument. An `async`
/// method runs in a span `<module>.call` at target `lua::lib::<module>`, with that name as `op`.
#[proc_macro_attribute]
pub fn lua_methods(args: TokenStream, item: TokenStream) -> TokenStream {
    let named = syn::parse_macro_input!(args as Named);
    let block = syn::parse_macro_input!(item as ItemImpl);
    lua_methods_impl(block, &named)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// What one method says above itself.
#[derive(Default)]
struct MethodWords {
    skip: bool,
    name: Option<LitStr>,
    role: Option<Ident>,
    doc: Option<Expr>,
}

impl MethodWords {
    fn above(attrs: &mut Vec<Attribute>) -> syn::Result<Self> {
        let mut words = Self::default();
        for attr in attrs.iter().filter(|a| a.path().is_ident("lua")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip") {
                    words.skip = true;
                } else if meta.path.is_ident("name") {
                    words.name = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("role") {
                    words.role = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("doc") {
                    words.doc = Some(meta.value()?.parse()?);
                } else {
                    return Err(meta.error(
                        "a method says `skip`, `name = \"...\"`, `role = ...` or `doc = ...`",
                    ));
                }
                Ok(())
            })?;
        }
        attrs.retain(|a| !a.path().is_ident("lua"));
        Ok(words)
    }
}

/// How a method's return reaches a script.
enum Handed<'a> {
    /// `Result<T, E>`: the value, or `nil` and the error as text.
    Fallible(&'a Type, &'a Type),
    /// `mlua::Result<T>`: the value, or the error raised.
    Raised(&'a Type),
    /// Anything else: the value.
    Plain(Tokens),
}

fn returned(output: &ReturnType) -> Handed<'_> {
    let ReturnType::Type(_, ty) = output else {
        return Handed::Plain(quote!(()));
    };
    if let Type::Path(path) = &**ty
        && let Some(last) = path.path.segments.last()
        && last.ident == "Result"
        && let PathArguments::AngleBracketed(given) = &last.arguments
    {
        let mut types = given.args.iter().filter_map(|arg| match arg {
            GenericArgument::Type(ty) => Some(ty),
            _ => None,
        });
        match (types.next(), types.next()) {
            (Some(value), Some(error)) => return Handed::Fallible(value, error),
            (Some(value), None) => return Handed::Raised(value),
            _ => {}
        }
    }
    Handed::Plain(ty.to_token_stream())
}

/// Whether a parameter's type is the engine (`&Lua`, `Lua`) and not an argument.
fn is_engine(ty: &Type) -> bool {
    let ty = match ty {
        Type::Reference(reference) => &*reference.elem,
        other => other,
    };
    matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|s| s.ident == "Lua"))
}

/// Whether a parameter's type is `Op`: the method's own name as its errors carry it (`bollard.logs`).
fn is_op(ty: &Type) -> bool {
    matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|s| s.ident == "Op"))
}

/// One method of a `lua_methods` block: its registration, and what the declarations say of it.
#[allow(clippy::too_many_lines)] // the generated registration, written out once
fn method_of(
    method: &ImplItemFn,
    words: MethodWords,
    prefix: &str,
    type_name: &str,
) -> syn::Result<(Tokens, Tokens)> {
    let facade = facade();
    let target = format!("lua::lib::{prefix}");
    let span = format!("{prefix}.call");
    let ident = &method.sig.ident;
    let name = words.name.map_or_else(|| ident.to_string(), |n| n.value());
    let op = format!("{prefix}.{name}");
    let doc = match (&words.doc, doc_of(&method.attrs)) {
        (Some(doc), _) => quote!(&::std::string::String::from(#doc)),
        (None, doc) => quote!(#doc),
    };
    let is_async = method.sig.asyncness.is_some();
    // what the method is given that a script does not pass, in the order it takes them
    let mut given = Vec::new();
    let mut params = Vec::new();
    for input in method.sig.inputs.iter().skip(1) {
        let FnArg::Typed(typed) = input else { continue };
        if params.is_empty() && is_engine(&typed.ty) {
            given.push(if is_async {
                quote!(__lua.clone())
            } else {
                quote!(__lua)
            });
            continue;
        }
        if params.is_empty() && is_op(&typed.ty) {
            given.push(quote!(#facade::args::Op(#op)));
            continue;
        }
        let Pat::Ident(binding) = &*typed.pat else {
            return Err(syn::Error::new(
                typed.pat.span(),
                "a parameter of a method a script calls has a name: it is the name the script knows it by",
            ));
        };
        params.push((binding.ident.clone(), &*typed.ty));
    }
    let idents: Vec<&Ident> = params.iter().map(|(ident, _)| ident).collect();
    let types: Vec<&Type> = params.iter().map(|(_, ty)| *ty).collect();
    let texts: Vec<String> = idents
        .iter()
        .map(|n| n.to_string().trim_start_matches("r#").to_owned())
        .collect();
    let returned = returned(&method.sig.output);
    let refuse = if let Handed::Fallible(_, error) = &returned {
        quote! {
            return #facade::args::refused(#op, <#error as ::std::convert::From<#facade::args::Refused>>::from(refused))
        }
    } else {
        quote! {
            return ::std::result::Result::Err(::mlua::Error::runtime(::std::format!("{}: {}", #op, refused)))
        }
    };
    // mlua counts a method's arguments from the receiver, and names the method with its type
    let places = 2..=idents.len() + 1;
    let raised_to = format!("{type_name}.{name}");
    let read = quote! {
        let mut __args = __args.into_iter();
        #(let #idents = match <#types as #facade::args::Argument>::read(
            __args.next().unwrap_or(::mlua::Value::Nil),
            &__lua,
        )
        // a value of the wrong type is raised as mlua raises it: with the argument's place and the method
        .map_err(|e| ::mlua::Error::BadArgument {
            to: ::std::option::Option::Some(::std::string::String::from(#raised_to)),
            pos: #places,
            name: ::std::option::Option::None,
            cause: ::std::sync::Arc::new(e),
        })? {
            ::std::result::Result::Ok(value) => value,
            ::std::result::Result::Err(refused) => {
                return ::std::result::Result::Ok(::std::result::Result::Err(refused));
            }
        };)*
        ::std::result::Result::Ok(::std::result::Result::Ok((#(#idents,)*)))
    };
    let wait = if is_async { quote!(.await) } else { quote!() };
    let call = || quote!(__this.#ident(#(#given,)* #(#idents),*) #wait);
    let hand = |called: Tokens| match &returned {
        Handed::Fallible(..) => quote! {
            match #called {
                ::std::result::Result::Ok(value) => crate::lua::sandbox::ok(value),
                ::std::result::Result::Err(e) => crate::lua::sandbox::from_err(#op, e),
            }
        },
        Handed::Raised(_) => called,
        Handed::Plain(_) => quote!(::std::result::Result::Ok(#called)),
    };
    let reading = quote! {
        let __read: ::mlua::Result<::std::result::Result<(#(#types,)*), #facade::args::Refused>> =
            (|| { #read })();
    };
    let taken = quote! {
        let (#(#idents,)*) = match __read? {
            ::std::result::Result::Ok(read) => read,
            ::std::result::Result::Err(refused) => { #refuse; }
        };
    };
    let add = if is_async {
        let role = words
            .role
            .unwrap_or_else(|| Ident::new("Wait", Span::call_site()));
        let handed = hand(call());
        quote! {
            crate::lua::lifecycle::async_method(
                methods,
                #name,
                crate::lua::lifecycle::Role::#role,
                |__lua: ::mlua::Lua, __this: ::mlua::UserDataRef<Self>, __args: ::mlua::MultiValue| {
                    #reading
                    let __this = <Self as ::std::clone::Clone>::clone(&__this);
                    ::tracing::Instrument::instrument(
                        async move {
                            #taken
                            #handed
                        },
                        ::tracing::info_span!(target: #target, #span, op = #op),
                    )
                },
            );
        }
    } else {
        let handed = hand(call());
        quote! {
            methods.add_method(#name, |__lua: &::mlua::Lua, __this: &Self, __args: ::mlua::MultiValue| {
                #reading
                #taken
                #handed
            });
        }
    };
    let ret = match &returned {
        Handed::Fallible(value, _) => {
            quote!(<crate::lua::sandbox::Fallible<#value> as #facade::DeclType>::decl_type())
        }
        Handed::Raised(value) => quote!(<#value as #facade::DeclType>::decl_type()),
        Handed::Plain(value) => quote!(<#value as #facade::DeclType>::decl_type()),
    };
    let note = quote! {{
        let params: ::std::vec::Vec<(&str, ::std::string::String)> =
            ::std::vec![#((#texts, <#types as #facade::DeclType>::decl_type())),*];
        let params: ::std::vec::Vec<(&str, &str)> = params.iter().map(|(n, t)| (*n, t.as_str())).collect();
        spec.method(#name, &params, &#ret, #doc);
    }};
    Ok((add, note))
}

fn lua_methods_impl(mut block: ItemImpl, named: &Named) -> syn::Result<Tokens> {
    let facade = facade();
    let Some((module, type_name)) = &named.0 else {
        return Err(syn::Error::new(
            block.self_ty.span(),
            "say the module the type is declared by and its name there: `#[lua_methods(\"std:time\", \"Duration\")]`",
        ));
    };
    let module_key = module.value();
    let prefix = module_key
        .split(':')
        .next_back()
        .unwrap_or(&module_key)
        .to_owned();
    let mut adds = Vec::new();
    let mut notes = Vec::new();
    for item in &mut block.items {
        let ImplItem::Fn(method) = item else { continue };
        let words = MethodWords::above(&mut method.attrs)?;
        let Some(FnArg::Receiver(_)) = method.sig.inputs.first() else {
            continue;
        };
        if words.skip {
            continue;
        }
        let (add, note) = method_of(method, words, &prefix, &type_name.value())?;
        adds.push(add);
        notes.push(note);
    }
    let ty = &block.self_ty;
    let (generics, _, where_clause) = block.generics.split_for_impl();
    Ok(quote! {
        #block

        impl #generics ::mlua::UserData for #ty #where_clause {
            fn add_methods<M: ::mlua::UserDataMethods<Self>>(methods: &mut M) {
                #(#adds)*
            }
        }

        impl #generics #ty #where_clause {
            /// What the declarations say of this type's methods: read from their signatures by `lua_methods`.
            pub(crate) fn lua(spec: &mut impl #facade::Members) {
                #(#notes)*
            }
        }

        impl #generics #facade::DeclType for #ty #where_clause {
            fn decl_type() -> ::std::string::String {
                ::std::format!("{}.{}", #facade::luau_decls::scope(#module), #type_name)
            }
        }

        impl #generics #facade::Named for #ty #where_clause {
            const MODULE: &'static str = #module;
            const NAME: &'static str = #type_name;
        }
    })
}

/// A struct that is a table to a script: `#[lua_record(into)]` for one a function hands a script,
/// `#[lua_record(from)]` for one a script passes (a table of options), or both. Its fields are the table's
/// keys, and the declaration is read from them (`{ code: number?, success: bool }`), so a function that
/// returns or takes the struct says nothing more.
///
/// A record read `from` a script is strict: a value that is not a table is refused, and so is a key that is
/// not a field, and a value that is not what its field takes. A field that may be left out is an `Option`. A
/// refusal is not raised: the function returns it as its own error (`facade::args`).
#[proc_macro_attribute]
pub fn lua_record(args: TokenStream, item: TokenStream) -> TokenStream {
    let record = syn::parse_macro_input!(item as syn::ItemStruct);
    lua_record_impl(args.into(), &record)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn lua_record_impl(args: Tokens, record: &syn::ItemStruct) -> syn::Result<Tokens> {
    let mut into = false;
    let mut from = false;
    let words = syn::meta::parser(|meta| {
        if meta.path.is_ident("into") {
            into = true;
        } else if meta.path.is_ident("from") {
            from = true;
        } else {
            return Err(meta.error("a record is `into` a script, `from` one, or both"));
        }
        Ok(())
    });
    syn::parse::Parser::parse2(words, args)?;
    if !into && !from {
        return Err(syn::Error::new(
            record.ident.span(),
            "say which way the record goes: `#[lua_record(into)]`, `#[lua_record(from)]` or both",
        ));
    }
    let syn::Fields::Named(fields) = &record.fields else {
        return Err(syn::Error::new(
            record.ident.span(),
            "a record is a struct with named fields: each is a key of the table",
        ));
    };
    let facade = facade();
    let ident = &record.ident;
    let (generics, of, bounds) = record.generics.split_for_impl();
    let names: Vec<&Ident> = fields.named.iter().flat_map(|f| &f.ident).collect();
    let keys: Vec<String> = names
        .iter()
        .map(|name| {
            let name = name.to_string();
            name.strip_prefix("r#").unwrap_or(&name).to_owned()
        })
        .collect();
    let types: Vec<&Type> = fields.named.iter().map(|f| &f.ty).collect();
    let count = keys.len();
    let mut out = quote! {
        #record

        impl #generics #facade::DeclType for #ident #of #bounds {
            fn decl_type() -> ::std::string::String {
                let fields: [::std::string::String; #count] = [
                    #(::std::format!("{}: {}", #keys, <#types as #facade::DeclType>::decl_type())),*
                ];
                ::std::format!("{{ {} }}", fields.join(", "))
            }
        }

        impl #generics #facade::Shape for #ident #of #bounds {
            const KEYS: &'static [&'static str] = &[#(#keys),*];
        }
    };
    if into {
        out.extend(quote! {
            impl #generics ::mlua::IntoLua for #ident #of #bounds {
                fn into_lua(self, lua: &::mlua::Lua) -> ::mlua::Result<::mlua::Value> {
                    let table = lua.create_table_with_capacity(0, #count)?;
                    #(table.raw_set(#keys, self.#names)?;)*
                    ::std::result::Result::Ok(::mlua::Value::Table(table))
                }
            }
        });
    }
    if from {
        out.extend(record_from(record, &keys, &names));
    }
    Ok(out)
}

/// What reads a record from a script: a table whose keys are all fields, each read as the field's type. A
/// wrong one is refused, not raised (`facade::args`).
fn record_from(record: &syn::ItemStruct, keys: &[String], names: &[&Ident]) -> Tokens {
    let facade = facade();
    let ident = &record.ident;
    let (generics, of, bounds) = record.generics.split_for_impl();
    let types = record.fields.iter().map(|f| &f.ty);
    quote! {
        impl #generics #ident #of #bounds {
            fn from_options(value: ::mlua::Value) -> ::std::result::Result<Self, #facade::args::Refused> {
                let table = #facade::args::options(value, <Self as #facade::Shape>::KEYS)?;
                ::std::result::Result::Ok(Self {
                    #(#names: <#types as #facade::args::Field>::field(
                        #keys,
                        #facade::args::value_of(&table, #keys),
                    )?,)*
                })
            }
        }

        impl #generics #facade::args::Argument for #ident #of #bounds {
            fn read(
                value: ::mlua::Value,
                _: &::mlua::Lua,
            ) -> ::mlua::Result<::std::result::Result<Self, #facade::args::Refused>> {
                ::std::result::Result::Ok(Self::from_options(value))
            }
        }

        impl #generics #facade::args::Field for #ident #of #bounds {
            fn field(
                _: &str,
                value: ::mlua::Value,
            ) -> ::std::result::Result<Self, #facade::args::Refused> {
                Self::from_options(value)
            }
        }
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
/// - `m.set("name", <a function made from a closure>)?`: the function is recorded after it. What it returns
///   is read from the closure's type, or is what `#[lua("...")]` says. A function made from something else
///   (a function item, a helper's result) says its return and its `takes(...)`;
/// - `m.set("name", <a value>)?` under `#[lua(constant)]`: a constant of the value's type, or of the type
///   `#[lua("type", constant)]` says.
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
    use crate::Named;
    use quote::quote;
    use syn::parse_quote;

    use super::{
        lua_fn_impl, lua_members_impl, lua_module_impl, lua_record_impl, lua_userdata_impl,
    };

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
    fn a_record_declares_its_fields_and_converts_the_way_it_says() {
        let record: syn::ItemStruct = parse_quote!(
            struct Exit {
                code: Option<i32>,
                r#type: String,
            }
        );
        let into = lua_record_impl(quote!(into), &record).map_or_default(|t| t.to_string());
        assert!(
            into.contains("\"{}: {}\" , \"code\" , < Option < i32 > as"),
            "{into}"
        );
        assert!(into.contains("\"type\""), "{into}");
        assert!(into.contains("IntoLua for Exit"), "{into}");
        assert!(!into.contains("Argument for Exit"), "{into}");
        let from = lua_record_impl(quote!(from), &record).map_or_default(|t| t.to_string());
        assert!(from.contains("args :: Argument for Exit"), "{from}");
        assert!(
            from.contains("args :: Field > :: field (\"type\""),
            "{from}"
        );
        refuses(
            lua_record_impl(quote!(), &record),
            "say which way the record goes",
        );
        refuses(
            lua_record_impl(quote!(sideways), &record),
            "`into` a script, `from` one",
        );
        let tuple: syn::ItemStruct = parse_quote!(
            struct Pair(i32, i32);
        );
        refuses(
            lua_record_impl(quote!(into), &tuple),
            "a struct with named fields",
        );
    }

    #[test]
    fn a_member_refuses_what_it_cannot_read() {
        let block = |body: proc_macro2::TokenStream| -> syn::ItemImpl {
            parse_quote!(impl UserData for X { fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) { #body } })
        };
        // what a closure returns is the compiler's to say: the closure goes through `typed`
        let inferred = lua_userdata_impl(
            block(quote!(methods.add_method("x", |_, this, n: f64| Ok(()));)),
            &Named::default(),
        )
        .map_or_default(|t| t.to_string());
        assert!(
            inferred.contains(
                "typed :: add_method :: < Self , _ , _ , _ , _ > (| _ , this , n : f64 | Ok (())"
            ),
            "{inferred}"
        );
        assert!(inferred.contains("name : \"x\""), "{inferred}");
        // a helper's closure is not one mlua is given: the name and the parameters are said, and what it
        // returns is left to the helper
        let left = lua_userdata_impl(
            block(quote!(#[lua(name = "x", takes())] helper(methods, |_, this, ()| Ok(()));)),
            &Named::default(),
        )
        .map_or_default(|t| t.to_string());
        assert!(
            left.contains("typed :: left (\"method\" , \"x\" , | |"),
            "{left}"
        );
        refuses(
            lua_userdata_impl(
                block(quote!(#[lua(name = "x")] helper(methods, |_, this, ()| Ok(()));)),
                &Named::default(),
            ),
            "say what a script gets",
        );
        refuses(
            lua_userdata_impl(
                block(quote!(#[lua("number")] methods.add_method("x", Self::helper);)),
                &Named::default(),
            ),
            "no closure to read the parameters from",
        );
        refuses(
            lua_userdata_impl(
                block(quote!(#[lua("number")] methods.add_method(NAME, |_, this, ()| Ok(1));)),
                &Named::default(),
            ),
            "the name of this member is not a string",
        );
        refuses(
            lua_userdata_impl(
                block(quote!(#[lua("number")] methods.document("x");)),
                &Named::default(),
            ),
            "what this statement adds cannot be read from it",
        );
        refuses(
            lua_userdata_impl(
                block(
                    quote!(#[lua("number", names(b = "by"))] methods.add_method("x", |_, this, a: f64| Ok(a));),
                ),
                &Named::default(),
            ),
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
        // a closure's return is the compiler's to say: the closure goes through `typed::returning`
        let inferred = lua_module_impl(declare(quote!(#[lua] m.set("one", make(|_, ()| Ok(1)))?;)))
            .map_or_default(|t| t.to_string());
        assert!(
            inferred.contains("typed :: returning (& __lua_ret , | _ , () | Ok (1))"),
            "{inferred}"
        );
        let waited = lua_module_impl(declare(
            quote!(#[lua] m.set("one", make(|_, ()| async move { Ok(1) }))?;),
        ))
        .map_or_default(|t| t.to_string());
        assert!(
            waited.contains("typed :: returning_async (& __lua_ret"),
            "{waited}"
        );
        // with no closure in sight there is nothing to read the return from
        refuses(
            lua_module_impl(declare(quote!(#[lua(takes())] m.set("now", now_fn)?;))),
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
        // a constant that does not say its type has its value's
        let inferred = lua_module_impl(declare(quote!(#[lua(constant)] m.set("os", OS)?;)))
            .map_or_default(|t| t.to_string());
        assert!(
            inferred.contains("typed :: constant (& __lua_ret , OS)"),
            "{inferred}"
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
        let out = lua_userdata_impl(
            block(quote!(
                /// Read from the output.
                #[lua("string?", name = "read", takes("max: number?"))]
                /// Read from the errors.
                #[lua(returns = RESULT, name = "read_stderr", takes(opts = OPTIONS), function)]
                for source in &SOURCES {
                    add_read(methods, source);
                }
            )),
            &Named::default(),
        );
        let text = out.map_or_default(|t| t.to_string());
        assert!(
            text.contains("typed :: said (\"method\" , \"read\""),
            "{text}"
        );
        assert!(text.contains("\"Read from the output.\""), "{text}");
        assert!(
            text.contains("typed :: said (\"function\" , \"read_stderr\""),
            "{text}"
        );
        assert!(text.contains("\"Read from the errors.\""), "{text}");
        assert!(
            text.contains("from (\"opts\") , :: std :: string :: String :: from (OPTIONS)"),
            "{text}"
        );
        assert!(!text.contains("# [lua"), "{text}");
        refuses(
            lua_userdata_impl(
                block(quote!(#[lua("number", name = "x")] helper!(methods, "x");)),
                &Named::default(),
            ),
            "no closure to read the parameters from",
        );
    }

    #[test]
    fn a_function_can_be_a_method_to_a_script() {
        let block = |body: proc_macro2::TokenStream| -> syn::ItemImpl {
            parse_quote!(impl UserData for X { fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) { #body } })
        };
        let out = lua_userdata_impl(
            block(quote!(
                #[lua("Duration", method, takes("other: Instant"))]
                methods.add_function("since", |_, args: (AnyUserData, AnyUserData)| Ok(1));
            )),
            &Named::default(),
        );
        let text = out.map_or_default(|t| t.to_string());
        assert!(
            text.contains("typed :: said (\"method\" , \"since\""),
            "{text}"
        );
        refuses(
            lua_userdata_impl(
                block(quote!(
                    #[lua("Duration", method)]
                    methods.add_function("since", |_, args: (AnyUserData, AnyUserData)| Ok(1));
                )),
                &Named::default(),
            ),
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
            lua_userdata_impl(
                in_methods(
                    quote!(#[lua("number", registered)] methods.add_method("x", |_, this, ()| Ok(1));),
                ),
                &Named::default(),
            ),
            "`registered` means nothing on a member",
        );
        refuses(
            lua_userdata_impl(
                in_methods(
                    quote!(#[lua("number", constant)] methods.add_method("x", |_, this, ()| Ok(1));),
                ),
                &Named::default(),
            ),
            "`constant` means nothing on a member",
        );
        refuses(
            lua_userdata_impl(
                in_methods(
                    quote!(#[lua("number", name = "x", registered, takes())] helper(methods);),
                ),
                &Named::default(),
            ),
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
            lua_userdata_impl(
                in_methods(
                    quote!(#[lua("number", method, function, takes())] methods.add_function("x", |_, ()| Ok(1));),
                ),
                &Named::default(),
            ),
            "this is said twice",
        );
        refuses(
            lua_userdata_impl(
                in_methods(quote!(
                    #[lua("number")]
                    #[lua(names(a = "b"))]
                    methods.add_method("x", |_, this, a: f64| Ok(a));
                )),
                &Named::default(),
            ),
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
            lua_userdata_impl(
                in_fields(
                    quote!(#[lua("number", takes("a: string"))] fields.add_field_method_get("x", |_, this| Ok(1));),
                ),
                &Named::default(),
            ),
            "a field takes no parameters",
        );
        refuses(
            lua_userdata_impl(
                in_fields(
                    quote!(#[lua("number", types(a = "string"))] fields.add_field_method_get("x", |_, this| Ok(1));),
                ),
                &Named::default(),
            ),
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
            lua_userdata_impl(
                in_methods(quote!(
                    #[lua("number", name = "x", takes())]
                    /// A doc below its mark.
                    helper(methods);
                )),
                &Named::default(),
            ),
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
