//! A parser of exactly what the emitter writes, so a test can refuse an invalid declarations file without
//! luau-lsp: the line forms of the file (comments, `export type`, `declare require`) and, for every type
//! text, a recursive-descent parse of the Luau type grammar the emitter uses (names, string literals,
//! tables, function types, parenthesised types, unions, intersections, `?`, `...T`).
//!
//! It is stricter than counting braces and narrower than Luau: a shape the emitter does not write is
//! refused here even when Luau would take it, so a new shape needs a decision, not a silent pass.

use std::collections::HashSet;

use super::{LUAU_KEYWORDS, is_ident};

/// Checks the whole definitions file. The first problem is the error, with its line (a control character
/// with its byte offset).
pub(crate) fn validate(text: &str) -> Result<(), String> {
    if let Some((at, c)) = text
        .char_indices()
        .find(|&(_, c)| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return Err(format!("control character {c:?} at byte {at}"));
    }
    let mut state = State::Top;
    let mut declared = HashSet::new();
    for (n, line) in text.split('\n').enumerate() {
        let at = |e: String| format!("line {}: {e}: `{line}`", n + 1);
        state = match state {
            State::Top => top(line, &mut declared).map_err(at)?,
            State::Type => in_type(line).map_err(at)?,
            State::Require { first } => in_require(line, first).map_err(at)?,
        };
    }
    match state {
        State::Top => Ok(()),
        _ => Err("the file ends inside a declaration".to_string()),
    }
}

enum State {
    Top,
    Type,
    Require { first: bool },
}

fn top(line: &str, declared: &mut HashSet<String>) -> Result<State, String> {
    if line.is_empty() || line == "--" || line.starts_with("-- ") {
        return Ok(State::Top);
    }
    if let Some(rest) = line.strip_prefix("export type ") {
        let (name, open) = rest
            .split_once(" = ")
            .ok_or("`export type` without ` = `")?;
        if !is_ident(name) {
            return Err(format!("`{name}` is not a type name"));
        }
        if !declared.insert(name.to_string()) {
            return Err(format!("`{name}` is declared twice"));
        }
        return match open {
            "{}" => Ok(State::Top),
            "{" => Ok(State::Type),
            _ => Err("a table type opens with `{`".to_string()),
        };
    }
    if line == "declare require: " {
        return Ok(State::Require { first: true });
    }
    Err("not a comment, an `export type` or `declare require:`".to_string())
}

fn in_type(line: &str) -> Result<State, String> {
    if line == "}" {
        return Ok(State::Top);
    }
    let body = line
        .strip_prefix('\t')
        .ok_or("a property or a doc line is indented by one tab")?;
    if body == "---" || body.starts_with("--- ") {
        return Ok(State::Type);
    }
    let (name, rest) = body.split_once(": ").ok_or("`name: type,` expected")?;
    let quoted = name
        .strip_prefix("[\"")
        .and_then(|n| n.strip_suffix("\"]"))
        .unwrap_or(name);
    if !is_ident(quoted) || (quoted == name && LUAU_KEYWORDS.contains(&name)) {
        return Err(format!(
            "`{name}` is not a property name (a reserved word is quoted)"
        ));
    }
    let ty = rest.strip_suffix(',').ok_or("a property ends with `,`")?;
    parse_type(ty)?;
    Ok(State::Type)
}

fn in_require(line: &str, first: bool) -> Result<State, String> {
    if line.is_empty() {
        return if first {
            Err("`declare require:` without an overload".to_string())
        } else {
            Ok(State::Top)
        };
    }
    let overload = if first {
        line.strip_prefix('\t')
    } else {
        line.strip_prefix("\t& ")
    }
    .ok_or("an overload is `\\t((path: \"ns:name\") -> type)`, then `\\t& ...`")?;
    parse_type(overload)?;
    Ok(State::Require { first: false })
}

#[derive(Debug, PartialEq)]
enum Tok {
    Id(String),
    Str,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Bar,
    Amp,
    Question,
    Arrow,
    Dots,
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        toks.push(match c {
            ' ' => continue,
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            '[' => Tok::LBracket,
            ']' => Tok::RBracket,
            '{' => Tok::LBrace,
            '}' => Tok::RBrace,
            ',' => Tok::Comma,
            ':' => Tok::Colon,
            '|' => Tok::Bar,
            '&' => Tok::Amp,
            '?' => Tok::Question,
            '-' if chars.get(i) == Some(&'>') => {
                i += 1;
                Tok::Arrow
            }
            '.' if chars[i..].starts_with(&['.', '.']) => {
                i += 2;
                Tok::Dots
            }
            '"' => {
                loop {
                    match chars.get(i) {
                        None => return Err("unterminated string".to_string()),
                        Some('\\') => i += 2,
                        Some('"') => break,
                        Some(_) => i += 1,
                    }
                }
                i += 1;
                Tok::Str
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i - 1;
                while chars
                    .get(i)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
                {
                    i += 1;
                }
                Tok::Id(chars[start..i].iter().collect())
            }
            other => return Err(format!("`{other}` is not part of a type")),
        });
    }
    Ok(toks)
}

/// Parses one type text; an error says what was expected.
fn parse_type(s: &str) -> Result<(), String> {
    let mut p = Parser {
        toks: lex(s)?,
        at: 0,
    };
    p.ty()?;
    if p.at == p.toks.len() {
        Ok(())
    } else {
        Err(format!("unexpected {:?} after the type", p.toks[p.at]))
    }
}

struct Parser {
    toks: Vec<Tok>,
    at: usize,
}

/// The shape of one parenthesised group.
struct Group {
    elements: usize,
    labelled: bool,
    last_is_variadic: bool,
    arrow: bool,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }

    fn peek_at(&self, n: usize) -> Option<&Tok> {
        self.toks.get(self.at + n)
    }

    /// Whether a name and a colon come next: a property or a parameter name.
    fn is_label(&self) -> bool {
        matches!(self.peek(), Some(Tok::Id(_))) && self.peek_at(1) == Some(&Tok::Colon)
    }

    /// Takes a property or parameter name: a reserved word is not one (it is written quoted or renamed).
    fn label(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(Tok::Id(word)) if LUAU_KEYWORDS.contains(&word.as_str()) => {
                Err(format!("`{word}` is a reserved word, not a label"))
            }
            Some(Tok::Id(_)) => {
                self.at += 1;
                Ok(())
            }
            other => Err(format!("expected a name, found {other:?}")),
        }
    }

    fn eat(&mut self, want: &Tok) -> bool {
        if self.peek() == Some(want) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, want: &Tok) -> Result<(), String> {
        if self.eat(want) {
            Ok(())
        } else {
            Err(format!("expected {want:?}, found {:?}", self.peek()))
        }
    }

    /// A union of intersections of optional primaries.
    fn ty(&mut self) -> Result<(), String> {
        self.inter()?;
        while self.eat(&Tok::Bar) {
            self.inter()?;
        }
        Ok(())
    }

    fn inter(&mut self) -> Result<(), String> {
        self.postfix()?;
        while self.eat(&Tok::Amp) {
            self.postfix()?;
        }
        Ok(())
    }

    fn postfix(&mut self) -> Result<(), String> {
        self.primary()?;
        while self.eat(&Tok::Question) {}
        Ok(())
    }

    fn primary(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(Tok::Id(_) | Tok::Str) => {
                self.at += 1;
                Ok(())
            }
            Some(Tok::LBrace) => self.table(),
            Some(Tok::LParen) => {
                let group = self.group()?;
                if group.arrow {
                    self.at += 1; // the arrow `group` saw
                    self.ret()
                } else if group.elements == 1 && !group.labelled && !group.last_is_variadic {
                    Ok(()) // a parenthesised type
                } else {
                    Err(
                        "a parenthesised group that is not a function type holds exactly one type"
                            .to_string(),
                    )
                }
            }
            other => Err(format!("expected a type, found {other:?}")),
        }
    }

    /// `{}`, `{ T }` (an array) or `{ name: T, [K]: V, ... }`.
    fn table(&mut self) -> Result<(), String> {
        self.expect(&Tok::LBrace)?;
        if self.eat(&Tok::RBrace) {
            return Ok(());
        }
        let is_entry = matches!(self.peek(), Some(Tok::LBracket)) || self.is_label();
        if !is_entry {
            self.ty()?; // `{ T }`
            return self.expect(&Tok::RBrace);
        }
        loop {
            if self.eat(&Tok::LBracket) {
                self.ty()?;
                self.expect(&Tok::RBracket)?;
            } else {
                self.label()?;
            }
            self.expect(&Tok::Colon)?;
            self.ty()?;
            if !self.eat(&Tok::Comma) || self.peek() == Some(&Tok::RBrace) {
                break;
            }
        }
        self.expect(&Tok::RBrace)
    }

    /// `( element, ... )`, where an element is `T`, `name: T` or `...T`; `arrow` says whether `->` follows
    /// (it is not consumed). `...T` must be last.
    fn group(&mut self) -> Result<Group, String> {
        self.expect(&Tok::LParen)?;
        let mut group = Group {
            elements: 0,
            labelled: false,
            last_is_variadic: false,
            arrow: false,
        };
        while self.peek() != Some(&Tok::RParen) {
            if group.last_is_variadic {
                return Err("`...` is not the last element".to_string());
            }
            if self.eat(&Tok::Dots) {
                group.last_is_variadic = true;
            } else if self.is_label() {
                self.label()?;
                self.expect(&Tok::Colon)?;
                group.labelled = true;
            }
            self.ty()?;
            group.elements += 1;
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RParen)?;
        group.arrow = self.peek() == Some(&Tok::Arrow);
        Ok(group)
    }

    /// What follows `->`: a pack `(A, B, ...C)` or `()`, `...T`, or one type.
    fn ret(&mut self) -> Result<(), String> {
        if self.eat(&Tok::Dots) {
            return self.ty();
        }
        if self.peek() != Some(&Tok::LParen) {
            return self.ty();
        }
        let group = self.group()?;
        if group.arrow {
            self.at += 1;
            return self.ret(); // a function type: `(a: A) -> B`
        }
        if group.labelled {
            return Err("a return pack has no labels".to_string());
        }
        if group.elements == 1 && !group.last_is_variadic {
            // `(T)` is a type: it may go on as `(T)?` or `(T) | U`
            while self.eat(&Tok::Question) {}
            while self.eat(&Tok::Bar) || self.eat(&Tok::Amp) {
                self.postfix()?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_type, validate};

    #[test]
    fn accepts_what_the_emitter_writes() {
        for ty in [
            "string",
            "string?",
            "{ [string]: any }",
            "{ status: number?, body: string?, headers: { [string]: string }? }",
            "{ string }",
            "\"GET\" | \"POST\"",
            "() -> ()",
            "(a: string, b: number?) -> (string?, string?)",
            "(self: lib_x_T, ...any) -> ...any",
            "(f: any) -> (any?, ...any)",
            "() -> (((a: string) -> number)?, string?)",
            "(handler: (req: vnd_hyper_Request) -> ...(string | { status: number? })) -> (nil, string?)",
            "((path: \"lib:json\") -> lib_json)",
            "((f))?",
            "() -> (nil, string?)",
        ] {
            assert_eq!(parse_type(ty), Ok(()), "{ty}");
        }
    }

    #[test]
    fn refuses_what_is_not_a_type() {
        for ty in [
            "((number, string)?, string?)", // a tuple inside a pack element, the `Result<(A, B)>` shape
            "(a: ())",                      // `()` is not the type of a value
            "(a: string",                   // unbalanced
            "string)",
            "{ end: number }",
            "(end: number) -> ()",
            "(a: string) ->",
            "(...any, a: string) -> ()", // `...` not last
            "string |",
            "(a: string, b: number)", // a group that is not a function
            "string\r",
            "\"open",
            "a.b",
        ] {
            assert!(parse_type(ty).is_err(), "{ty} was accepted");
        }
    }

    #[test]
    fn validates_a_file() {
        let good = "-- comment\n\nexport type a_B = {\n\t--- doc\n\tf: (x: number) -> (),\n\t[\"end\"]: string,\n}\n\nexport type a_C = {}\n\ndeclare require: \n\t((path: \"a:b\") -> a_B)\n\t& ((path: \"a:c\") -> a_C)\n";
        assert_eq!(validate(good), Ok(()));
    }

    #[test]
    fn refuses_a_bad_file() {
        let doc = "export type a_B = {\n\t--- doc\n\tf: () -> (),\n}\n";
        assert!(validate(doc).is_ok());
        let cases = [
            (
                "a doc with a bare CR",
                "export type a_B = {\n\t--- one\rtwo\n}\n",
            ),
            (
                "a property without a comma",
                "export type a_B = {\n\tf: () -> ()\n}\n",
            ),
            ("an unclosed table", "export type a_B = {\n\tf: () -> (),\n"),
            (
                "a bad type",
                "export type a_B = {\n\tf: (number, string)?,\n}\n",
            ),
            ("a stray line", "nonsense\n"),
            (
                "a bad property name",
                "export type a_B = {\n\tend: number,\n}\n",
            ),
            (
                "a twice declared name",
                "export type a_B = {}\nexport type a_B = {}\n",
            ),
            ("a nested line", "export type a_B = {\n\t\tf: number,\n}\n"),
            ("require without an overload", "declare require: \n\n"),
        ];
        for (what, text) in cases {
            assert!(validate(text).is_err(), "{what} was accepted: {text:?}");
        }
    }
}
