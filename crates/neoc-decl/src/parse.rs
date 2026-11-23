//! Reads Luau declarations into the tree of this crate: a whole definitions file ([`sections`]: its `-- ===== <name>
//! =====` sections, the `export type` aliases and `declare`d globals of each), or one type as Luau writes it ([`ty`],
//! [`ret`]).
//!
//! The grammar is the part of Luau's type syntax the declarations use: names, string literals, `T?`, unions and
//! intersections, `{ T }`, table types with properties and an indexer, function types (generic, variadic, with
//! several returns) and parentheses. Anything else is an error that names the line, never a declaration left out.

use crate::{Decl, Func, Param, Prop, Ret, Section, Ty};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Name(String),
    Str(String),
    Sym(&'static str),
    /// Consecutive `---` lines, joined with `\n`.
    Doc(String),
    /// `-- ===== <name> =====`
    Section(String),
}

/// Longest first: `...` before `.`, `->` before nothing else that starts with `-`.
const SYMBOLS: &[&str] = &[
    "...", "->", "(", ")", "{", "}", "[", "]", "<", ">", ",", ";", ":", "|", "&", "?", "=", ".",
];

fn lex(text: &str) -> Result<Vec<(Tok, usize)>, String> {
    let mut out: Vec<(Tok, usize)> = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let mut rest = raw.trim_start();
        if let Some(doc) = rest.strip_prefix("---") {
            let doc = doc.strip_prefix(' ').unwrap_or(doc).trim_end();
            match out.last_mut() {
                // a doc line continues the doc above it only when nothing came between them
                Some((Tok::Doc(above), at)) if *at + 1 == line => {
                    above.push('\n');
                    above.push_str(doc);
                    *at = line;
                }
                _ => out.push((Tok::Doc(doc.to_owned()), line)),
            }
            continue;
        }
        if let Some(name) = rest
            .strip_prefix("-- ===== ")
            .and_then(|r| r.trim_end().strip_suffix(" ====="))
        {
            out.push((Tok::Section(name.to_owned()), line));
            continue;
        }
        while !rest.is_empty() {
            if rest.starts_with("--") {
                break;
            }
            let first = rest.chars().next().unwrap_or(' ');
            if first.is_whitespace() {
                rest = rest.trim_start();
            } else if first.is_ascii_alphabetic() || first == '_' {
                let end = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(rest.len());
                out.push((Tok::Name(rest[..end].to_owned()), line));
                rest = &rest[end..];
            } else if first == '"' {
                // to the quote that closes it: a backslash takes the character after it with it
                let mut end = None;
                let mut escaped = false;
                for (at, c) in rest.char_indices().skip(1) {
                    match (escaped, c) {
                        (true, _) => escaped = false,
                        (false, '\\') => escaped = true,
                        (false, '"') => {
                            end = Some(at);
                            break;
                        }
                        _ => {}
                    }
                }
                let Some(end) = end else {
                    return Err(format!("line {line}: a string does not end"));
                };
                out.push((Tok::Str(rest[..=end].to_owned()), line));
                rest = &rest[end + 1..];
            } else if let Some(symbol) = SYMBOLS.iter().find(|s| rest.starts_with(**s)) {
                out.push((Tok::Sym(symbol), line));
                rest = &rest[symbol.len()..];
            } else {
                return Err(format!(
                    "line {line}: `{first}` is not part of a declaration"
                ));
            }
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<(Tok, usize)>,
    at: usize,
}

type Parse<T> = Result<T, String>;

/// One entry between the parentheses of a function's parameters, a tuple of returns or a parenthesised type.
enum Item {
    Value(Param),
    Rest(Ty),
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at).map(|(tok, _)| tok)
    }

    fn peek_at(&self, ahead: usize) -> Option<&Tok> {
        self.toks.get(self.at + ahead).map(|(tok, _)| tok)
    }

    fn line(&self) -> usize {
        self.toks
            .get(self.at)
            .or_else(|| self.toks.last())
            .map_or(0, |(_, line)| *line)
    }

    fn fail<T>(&self, what: &str) -> Parse<T> {
        let found = match self.peek() {
            Some(Tok::Name(name)) => format!("`{name}`"),
            Some(Tok::Str(text)) => text.clone(),
            Some(Tok::Sym(symbol)) => format!("`{symbol}`"),
            Some(Tok::Doc(_)) => "a `---` comment".to_owned(),
            Some(Tok::Section(name)) => format!("the section `{name}`"),
            None => "the end of the file".to_owned(),
        };
        Err(format!("line {}: {what}, found {found}", self.line()))
    }

    fn is(&self, symbol: &str) -> bool {
        matches!(self.peek(), Some(Tok::Sym(s)) if *s == symbol)
    }

    fn eat(&mut self, symbol: &str) -> bool {
        let found = self.is(symbol);
        if found {
            self.at += 1;
        }
        found
    }

    fn expect(&mut self, symbol: &str) -> Parse<()> {
        if self.eat(symbol) {
            Ok(())
        } else {
            self.fail(&format!("expected `{symbol}`"))
        }
    }

    fn name(&mut self) -> Parse<String> {
        match self.peek() {
            Some(Tok::Name(name)) => {
                let name = name.clone();
                self.at += 1;
                Ok(name)
            }
            _ => self.fail("expected a name"),
        }
    }

    /// The `---` lines above what comes next (the last block of them, when a blank line parts two).
    fn doc(&mut self) -> String {
        let mut found = String::new();
        while let Some(Tok::Doc(doc)) = self.peek() {
            found.clone_from(doc);
            self.at += 1;
        }
        found
    }

    /// Nothing is left to read of `text`.
    fn end(&self, text: &str) -> Parse<()> {
        if self.peek().is_some() {
            return self.fail(&format!("`{text}` goes on after the type"));
        }
        Ok(())
    }

    fn sections(&mut self) -> Parse<Vec<Section>> {
        let mut sections: Vec<Section> = Vec::new();
        while self.peek().is_some() {
            if let Some(Tok::Section(name)) = self.peek() {
                sections.push(Section {
                    name: name.clone(),
                    aliases: Vec::new(),
                    globals: Vec::new(),
                });
                self.at += 1;
                continue;
            }
            let doc = self.doc();
            let Some(Tok::Name(keyword)) = self.peek().cloned() else {
                return self.fail("expected `export type` or `declare`");
            };
            self.at += 1;
            let global = match keyword.as_str() {
                "declare" => true,
                "export" if matches!(self.peek(), Some(Tok::Name(n)) if n == "type") => {
                    self.at += 1;
                    false
                }
                _ => {
                    self.at -= 1;
                    return self.fail("expected `export type` or `declare`");
                }
            };
            let name = self.name()?;
            self.expect(if global { ":" } else { "=" })?;
            let decl = Decl {
                doc,
                name,
                ty: self.ty(None)?,
            };
            let Some(section) = sections.last_mut() else {
                return Err(format!(
                    "`{}` is declared before the first `-- ===== <name> =====` line",
                    decl.name
                ));
            };
            if global {
                section.globals.push(decl);
            } else {
                section.aliases.push(decl);
            }
        }
        Ok(sections)
    }

    /// A type. `first` is its first operand when the caller has already read it (a parenthesised type or a
    /// function, found while reading what could have been a tuple of returns).
    fn ty(&mut self, first: Option<Ty>) -> Parse<Ty> {
        let mut parts = vec![self.inter(first)?];
        while self.eat("|") {
            parts.push(self.inter(None)?);
        }
        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            Ty::Union(parts)
        })
    }

    fn inter(&mut self, first: Option<Ty>) -> Parse<Ty> {
        let mut parts = vec![self.postfix(first)?];
        while self.eat("&") {
            parts.push(self.postfix(None)?);
        }
        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            Ty::Inter(parts)
        })
    }

    fn postfix(&mut self, first: Option<Ty>) -> Parse<Ty> {
        let mut ty = match first {
            Some(ty) => ty,
            None => self.primary()?,
        };
        while self.eat("?") {
            ty = Ty::Optional(Box::new(ty));
        }
        Ok(ty)
    }

    fn primary(&mut self) -> Parse<Ty> {
        match self.peek().cloned() {
            Some(Tok::Str(text)) => {
                self.at += 1;
                Ok(Ty::Str(text))
            }
            Some(Tok::Name(name)) => {
                self.at += 1;
                if self.is(".") || self.is("<") {
                    return self.fail(&format!("`{name}` is followed by what is not translated"));
                }
                Ok(match name.as_str() {
                    "true" => Ty::Bool(true),
                    "false" => Ty::Bool(false),
                    "typeof" => return self.fail("`typeof` is not translated"),
                    _ => Ty::Name(name),
                })
            }
            Some(Tok::Sym("{")) => self.table(),
            Some(Tok::Sym("<")) => {
                self.at += 1;
                let mut generics = vec![self.name()?];
                while self.eat(",") {
                    generics.push(self.name()?);
                }
                self.expect(">")?;
                let items = self.items()?;
                self.expect("->")?;
                self.func(generics, items)
                    .map(|func| Ty::Func(Box::new(func)))
            }
            Some(Tok::Sym("(")) => {
                let items = self.items()?;
                if self.eat("->") {
                    return self
                        .func(Vec::new(), items)
                        .map(|func| Ty::Func(Box::new(func)));
                }
                self.one(items)
            }
            _ => self.fail("expected a type"),
        }
    }

    /// `( a: A, B, ...C )`
    fn items(&mut self) -> Parse<Vec<Item>> {
        self.expect("(")?;
        let mut items = Vec::new();
        while !self.is(")") {
            if matches!(items.last(), Some(Item::Rest(_))) {
                return self.fail("`...` is not last");
            }
            if self.eat("...") {
                items.push(Item::Rest(self.postfix(None)?));
            } else if matches!(
                (self.peek(), self.peek_at(1)),
                (Some(Tok::Name(_)), Some(Tok::Sym(":")))
            ) {
                let name = self.name()?;
                self.expect(":")?;
                items.push(Item::Value(Param {
                    name: Some(name),
                    ty: self.ty(None)?,
                }));
            } else {
                items.push(Item::Value(Param {
                    name: None,
                    ty: self.ty(None)?,
                }));
            }
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")")?;
        Ok(items)
    }

    /// The type inside a pair of parentheses that is not a function's parameters.
    fn one(&mut self, mut items: Vec<Item>) -> Parse<Ty> {
        match (items.pop(), items.is_empty()) {
            (Some(Item::Value(Param { name: None, ty })), true) => Ok(ty),
            _ => self.fail("expected `->` after the parameters of a function"),
        }
    }

    fn func(&mut self, generics: Vec<String>, items: Vec<Item>) -> Parse<Func> {
        let mut params = Vec::new();
        let mut rest = None;
        for item in items {
            match item {
                Item::Value(param) => params.push(param),
                Item::Rest(ty) => rest = Some(ty),
            }
        }
        Ok(Func {
            generics,
            params,
            rest,
            ret: self.ret()?,
        })
    }

    fn ret(&mut self) -> Parse<Ret> {
        if self.eat("...") {
            return Ok(Ret {
                values: Vec::new(),
                rest: Some(self.postfix(None)?),
            });
        }
        if !self.is("(") {
            return Ok(Ret {
                values: vec![self.ty(None)?],
                rest: None,
            });
        }
        let items = self.items()?;
        // a function is returned, or a parenthesised type goes on: `(A | B)?`, `(A) -> B`
        if self.eat("->") {
            let func = self.func(Vec::new(), items)?;
            return Ok(Ret {
                values: vec![self.ty(Some(Ty::Func(Box::new(func))))?],
                rest: None,
            });
        }
        if self.is("?") || self.is("|") || self.is("&") {
            let inner = self.one(items)?;
            return Ok(Ret {
                values: vec![self.ty(Some(inner))?],
                rest: None,
            });
        }
        let mut values = Vec::new();
        let mut rest = None;
        for item in items {
            match item {
                Item::Value(Param { name: None, ty }) => values.push(ty),
                Item::Value(Param {
                    name: Some(name), ..
                }) => {
                    return self.fail(&format!("a return is named `{name}`"));
                }
                Item::Rest(ty) => rest = Some(ty),
            }
        }
        Ok(Ret { values, rest })
    }

    fn table(&mut self) -> Parse<Ty> {
        self.expect("{")?;
        let keyed = matches!(
            (self.peek(), self.peek_at(1)),
            (Some(Tok::Doc(_) | Tok::Sym("[" | "}")), _)
                | (Some(Tok::Name(_)), Some(Tok::Sym(":")))
        );
        if !keyed {
            let item = self.ty(None)?;
            self.expect("}")?;
            return Ok(Ty::Array(Box::new(item)));
        }
        let mut props = Vec::new();
        let mut index = None;
        while !self.is("}") {
            let doc = self.doc();
            if self.eat("[") {
                if let (Some(Tok::Str(text)), Some(Tok::Sym("]"))) =
                    (self.peek().cloned(), self.peek_at(1))
                {
                    self.at += 2;
                    self.expect(":")?;
                    props.push(Prop {
                        doc,
                        name: text.trim_matches('"').to_owned(),
                        ty: self.ty(None)?,
                    });
                } else {
                    let key = self.ty(None)?;
                    self.expect("]")?;
                    self.expect(":")?;
                    if index.replace(Box::new((key, self.ty(None)?))).is_some() {
                        return self.fail("a table type has two indexers");
                    }
                }
            } else {
                let name = self.name()?;
                self.expect(":")?;
                props.push(Prop {
                    doc,
                    name,
                    ty: self.ty(None)?,
                });
            }
            if !(self.eat(",") || self.eat(";")) {
                break;
            }
        }
        self.expect("}")?;
        Ok(Ty::Table { props, index })
    }
}

/// The sections of a Luau definitions file, in order.
///
/// # Errors
/// The text uses what the grammar above does not have. The message names the line.
pub fn sections(luau: &str) -> Result<Vec<Section>, String> {
    Parser {
        toks: lex(luau)?,
        at: 0,
    }
    .sections()
}

/// One type, as Luau writes it: `{ [string]: number }?`.
///
/// # Errors
/// As [`sections`], and when anything follows the type.
pub fn ty(luau: &str) -> Result<Ty, String> {
    let mut parser = Parser {
        toks: lex(luau)?,
        at: 0,
    };
    let ty = parser.ty(None)?;
    parser.end(luau)?;
    Ok(ty)
}

/// What a function returns, as Luau writes it after `->`: `()`, `string`, `(string?, string?)`, `...any`.
///
/// # Errors
/// As [`ty`].
pub fn ret(luau: &str) -> Result<Ret, String> {
    let mut parser = Parser {
        toks: lex(luau)?,
        at: 0,
    };
    let ret = parser.ret()?;
    parser.end(luau)?;
    Ok(ret)
}
