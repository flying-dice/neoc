# Lexical structure

Defines the alphabet of neoc: what characters source files are made of, how those characters cluster into tokens, and which token sequences the rest of the spec then operates on.

The grammar in `SYNTAX.md` consumes the token stream produced by this chapter, not raw source text.

Notation: EBNF as defined in `NOTATION.md`. Token classes here are referenced as `UPPER_SNAKE_CASE` non-terminals from `SYNTAX.md`.

---

## 1. Source representation

A source file is a sequence of Unicode code points encoded as UTF-8. The file extension is `.neoc`.

Line endings are LF (`U+000A`) or CRLF (`U+000D U+000A`); a CRLF is treated everywhere as a single LF. A file may end with or without a final line ending.

There is no byte-order mark. A leading BOM is a lexical error.

There is no maximum line length and no significant column position; indentation carries no meaning (no off-side rule).

## 2. Whitespace

The following code points are whitespace: horizontal tab `U+0009`, line feed `U+000A`, carriage return `U+000D`, and space `U+0020`.

Whitespace separates tokens and is otherwise insignificant. Any run of whitespace is equivalent to a single space, except inside string literals where it is preserved verbatim.

```
WHITESPACE ::= { "\t" | "\n" | "\r" | " " }+
```

## 3. Comments

```
LINE_COMMENT  ::= "//" { ANY - NEWLINE }
BLOCK_COMMENT ::= "/*" { BLOCK_COMMENT | ANY } "*/"   (* nests *)
DOC_LINE      ::= "///" { ANY - NEWLINE }
DOC_BLOCK     ::= "/**" { BLOCK_COMMENT | ANY } "*/"
```

- **Line comments** start at `//` and run to the end of the line.
- **Block comments** are delimited by `/*` and `*/` and **nest**: every `/*` must be matched by a `*/`.
- **Doc comments** are attached to the item that immediately follows them. `///` and `/**…*/` document the following item; they are sugar for the `doc` attribute (`SYNTAX.md` §9). A doc comment in any other position is a lexical error.

Ordinary comments (`//`, `/* */`) are discarded by the lexer and never appear in the token stream. Doc comments are retained and attached.

## 4. Identifiers

```
IDENT      ::= ID_START { ID_CONTINUE } - KEYWORD
ID_START   ::= "_" | UNICODE_XID_START
ID_CONTINUE ::= UNICODE_XID_CONTINUE
```

`UNICODE_XID_START` and `UNICODE_XID_CONTINUE` are the character classes defined by Unicode UAX #31. In the common ASCII case this is `[A-Za-z_]` to start and `[A-Za-z0-9_]` to continue.

Identifiers are compared by Unicode Normalization Form C. They are case-sensitive. The single underscore `_` is not an identifier; it is the wildcard token (`WILDCARD`).

No identifier may spell a keyword (§5). Casing convention is not enforced by the lexer, but the recommended style is `snake_case` for values and functions, `UpperCamelCase` for types, traits, and enum variants, and `SCREAMING_SNAKE_CASE` for constants.

## 5. Keywords

The following are reserved and may not be used as identifiers:

```
as      break   const   continue  else    enum    ext     fn
for     if      impl    in        let     loop    match   mod
pub     return  self    Self      trait   type    use     where
while
```

Boolean literals `true` and `false` (§6.4) are also reserved.

The following are reserved for future use and may not be used as identifiers, though they have no meaning yet:

```
async   await   crate   dyn     move    static  super   unsafe
```

`class` is a **contextual** keyword: it is a keyword only as an `ext`/`extern` block item (`SYNTAX.md`, `ExtItem`), where `class IDENT ;` imports a foreign class. Everywhere else it is an ordinary identifier and is not reserved.

`number`, `string`, and `bool` are **not** keywords; they are predeclared type names (`STATIC.md` §3.1) living in the type namespace, and may be shadowed only by a `type` alias or `struct`/`enum` of the same name (which is discouraged).

## 6. Literals

### 6.1 Integer literals

```
INT_LITERAL ::= DEC_INT | HEX_INT | OCT_INT | BIN_INT
DEC_INT     ::= DIGIT { DIGIT | "_" }
HEX_INT     ::= "0x" HEX_DIGIT { HEX_DIGIT | "_" }
OCT_INT     ::= "0o" OCT_DIGIT { OCT_DIGIT | "_" }
BIN_INT     ::= "0b" BIN_DIGIT { BIN_DIGIT | "_" }
DIGIT       ::= "0".."9"
HEX_DIGIT   ::= DIGIT | "a".."f" | "A".."F"
OCT_DIGIT   ::= "0".."7"
BIN_DIGIT   ::= "0" | "1"
```

Underscores are digit separators and carry no meaning: `1_000_000`. A literal may not start or end with `_` (the leading character is a digit; a trailing `_` is permitted by the grammar above and normalised away).

There are no integer-width suffixes (`i32`, `u8`, …). Every integer literal has type `number` (`STATIC.md` §3.1) — neoc has a single numeric type because its target has one.

### 6.2 Floating-point literals

```
FLOAT_LITERAL ::= DEC_INT "." DEC_INT [ EXPONENT ]
                | DEC_INT EXPONENT
EXPONENT      ::= ( "e" | "E" ) [ "+" | "-" ] DEC_INT
```

A float literal requires digits on both sides of the `.` (`1.0`, not `1.` or `.5`). Like integers, every float literal has type `number`; the distinction is purely lexical.

### 6.3 String literals

```
STRING_LITERAL ::= '"' { STRING_CHAR | ESCAPE } '"'
STRING_CHAR    ::= ANY - ( '"' | "\" )
ESCAPE         ::= "\" ( "n" | "t" | "r" | "\" | '"' | "0"
                       | "x" HEX_DIGIT HEX_DIGIT
                       | "u{" HEX_DIGIT { HEX_DIGIT } "}" )
RAW_STRING     ::= "r" RAW_BODY
RAW_BODY       ::= '"' { ANY - '"' } '"'
                 | "#" RAW_BODY "#"
```

Strings are double-quoted. Single quotes are not string delimiters and have no lexical meaning. A string may span multiple lines; embedded newlines are part of the value.

Escape sequences: `\n`, `\t`, `\r`, `\\`, `\"`, `\0`, hexadecimal `\xNN`, and Unicode `\u{NNNN}`.

Raw strings are prefixed with `r` and suppress all escape processing: `r"C:\path"`. To include a `"`, add matching `#` hashes: `r#"he said "hi""#`.

Every string literal has type `string`.

### 6.4 Boolean literals

```
BOOL_LITERAL ::= "true" | "false"
```

`true` and `false` are reserved words that also serve as the two values of type `bool`.

## 7. Operators and punctuation

The lexer uses maximal munch: the longest matching token wins, so `..=` lexes as one token, not `..` followed by `=`.

```
PLUS    "+"     MINUS   "-"     STAR    "*"     SLASH   "/"     PERCENT "%"
EQ      "=="    NE      "!="    LT      "<"     GT      ">"     LE      "<="
GE      ">="    AND     "&&"    OR      "||"    NOT     "!"
ASSIGN  "="     PLUSEQ  "+="    MINUSEQ "-="    STAREQ  "*="    SLASHEQ "/="
PERCENTEQ "%="
DOT     "."     COMMA   ","     COLON   ":"     SEMI    ";"     ARROW   "->"
FATARROW "=>"   PATH    "::"    QUESTION "?"    PIPE    "|"     AMP     "&"
DOTDOT  ".."    DOTDOTEQ "..="  AT      "@"     POUND   "#"     WILDCARD "_"
LPAREN  "("     RPAREN  ")"     LBRACE  "{"     RBRACE  "}"     LBRACK  "["
RBRACK  "]"
```

Notes:

- `=>` is the match-arm separator. It is not a closure arrow (closures use `|params|`, §`SYNTAX.md`).
- `|` is both the bitwise/pattern-alternation token and the closure-parameter delimiter; context in the grammar disambiguates.
- `&` exists as a token but neoc has no reference types; its only uses are the `&&` operator (a distinct token) and reserved future syntax. A bare `&` in a type or expression position is a syntax error.
- There is no `===`, `!==`, `??`, `||=`, `&&=`, `??=`, `...`, `?.`, or backtick. Those are not neoc tokens.

## 8. Token grammar

The complete set of token classes the grammar in `SYNTAX.md` refers to:

```
(* literals *)
INT_LITERAL  FLOAT_LITERAL  STRING_LITERAL  BOOL_LITERAL

(* names *)
IDENT                       (* §4 — values, fields, type params, lifetimes(none) *)

(* keywords — §5, each its own terminal spelled as its source text *)
"as" "break" "const" "continue" "else" "enum" "ext" "fn" "for" "if"
"impl" "in" "let" "loop" "match" "mod" "pub" "return" "self" "Self"
"trait" "type" "use" "where" "while"

(* punctuation and operators — §7, each its own terminal *)
"+" "-" "*" "/" "%" "==" "!=" "<" ">" "<=" ">=" "&&" "||" "!"
"=" "+=" "-=" "*=" "/=" "%="
"." "," ":" ";" "->" "=>" "::" "?" "|" "&" ".." "..=" "@" "#" "_"
"(" ")" "{" "}" "[" "]"
```

`number`, `string`, `bool` are ordinary `IDENT`s at the lexical layer; their special status is a name-resolution rule in `STATIC.md`, not a token kind.
