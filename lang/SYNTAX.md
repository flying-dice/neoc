# Syntax

Defines which token sequences are valid neoc programs. Operates on the tokens defined by `LEXICAL.md`; says nothing about meaning — that belongs to `STATIC.md` and `DYNAMIC.md`.

Notation: EBNF as defined in `NOTATION.md`. Token classes are written `UPPER_SNAKE_CASE` and resolve to the productions in `LEXICAL.md` §8. Keyword and punctuation literals are written as their source spelling: `"struct"`, `"->"`, `"::"`.

This file is the **canonical** syntactic definition of neoc. Any parser implementation — the tree-sitter grammar in `crates/neoc-grammar/`, a future hand-rolled parser, anything in another host language — must accept exactly the language defined here. Where parser and spec disagree, the spec wins.

---

## 1. Source files

```
SourceFile ::= { Item }
```

A source file is a sequence of top-level items. The file is its own module; its path determines its module identity (`STATIC.md` §2). There is no top-level statement or expression position — executable code lives inside `fn` bodies.

## 2. Items

```
Item ::= [ Attribute ] [ "pub" ] ItemKind

ItemKind ::= UseDecl
           | ConstDecl
           | TypeAlias
           | StructDecl
           | EnumDecl
           | TraitDecl
           | ImplBlock
           | FnDecl
           | ExtBlock
```

`pub` marks the item exported from its module; its absence makes the item module-private. Attributes (§8) precede the optional `pub`.

## 3. Declarations

### 3.1 `use`

```
UseDecl ::= "use" UsePath [ "as" IDENT ] ";"
UsePath ::= IDENT { "::" IDENT }
```

Brings a name from another module into scope. `use math::vec::Vec3;` makes `Vec3` available unqualified; `use math::vec::Vec3 as V;` binds it under an alias.

### 3.2 `const`

```
ConstDecl ::= "const" IDENT ":" Type "=" Expr ";"
```

A module- or block-level constant. The initialiser is an expression evaluated once; its type is mandatory.

### 3.3 `type` alias

```
TypeAlias ::= "type" IDENT [ Generics ] "=" Type ";"
```

Introduces a new spelling for an existing type. Aliases are transparent — `type Id = number;` makes `Id` and `number` interchangeable. There are no conditional, mapped, or union type aliases.

### 3.4 `struct`

```
StructDecl ::= "struct" IDENT [ Generics ] StructTail
StructTail ::= ";"                              (* unit struct  *)
             | "(" Type ")" ";"                 (* newtype      *)
             | "{" [ FieldList ] "}"            (* named fields *)

FieldList  ::= Field { "," Field } [ "," ]
Field      ::= { Attribute } IDENT ":" Type
```

Three forms, mirroring the value forms in §6.4:

- **Unit** — `struct Marker;` — no fields.
- **Newtype** — `struct UserId(number);` — exactly one positional field, accessed as `.0`.
- **Named** — `struct Point { x: number, y: number }`.

Field attributes (e.g. validation constraints) precede the field name.

### 3.5 `enum`

```
EnumDecl    ::= "enum" IDENT [ Generics ] "{" [ VariantList ] "}"
VariantList ::= Variant { "," Variant } [ "," ]
Variant     ::= { Attribute } IDENT VariantTail
VariantTail ::= (* empty *)                     (* unit variant   *)
              | "(" TypeList ")"                 (* tuple variant  *)
              | "{" [ FieldList ] "}"            (* struct variant *)
TypeList    ::= Type { "," Type } [ "," ]
```

The sum type. A value of an enum is exactly one of its variants. Variants come in the same three shapes as structs: unit (`Red`), tuple (`Move(number, number)`), struct (`Circle { radius: number }`).

neoc has no anonymous union types. A sum is always a named `enum`; there is no `A | B` type. `Result` and `Option` are ordinary library enums (`STATIC.md` §3.6).

### 3.6 `trait`

```
TraitDecl   ::= "trait" IDENT [ Generics ] "{" { TraitItem } "}"
TraitItem   ::= FnSig ";"                        (* required method *)
              | FnSig Block                       (* default method  *)
FnSig       ::= "fn" IDENT [ Generics ] "(" [ ParamList ] ")" [ "->" Type ]
```

A trait is a set of method signatures. A method with a body is a default; an implementing type may override it. A method without a body is required.

### 3.7 `impl`

```
ImplBlock  ::= "impl" [ Generics ] ImplHead "{" { ImplItem } "}"
ImplHead   ::= Type                              (* inherent impl *)
             | TypePath "for" Type                (* trait impl    *)
ImplItem   ::= [ Attribute ] FnDecl
```

An inherent impl (`impl Point { … }`) attaches methods and associated functions to a type. A trait impl (`impl Display for Point { … }`) provides that type's implementation of a trait.

### 3.8 `fn`

```
FnDecl    ::= "fn" IDENT [ Generics ] "(" [ ParamList ] ")" [ "->" Type ] Block
ParamList ::= Param { "," Param } [ "," ]
Param     ::= SelfParam | IDENT ":" Type
SelfParam ::= "self"
```

A function. Inside an impl, the first parameter may be `self`, making it a method; otherwise it is an associated function called through the type path (`Point::origin()`).

There is one self form: `self`. neoc has no references, so there is no `&self` or `&mut self` — a method receives the value, and because the runtime's values are reference-shared, mutation through `self` is visible to the caller. This is the central simplification over Rust: no borrowing, no lifetimes.

### 3.9 `ext`

```
ExtBlock ::= "ext" "{" { ExtItem } "}"
           | "ext" ExtItem
ExtItem  ::= "fn" IDENT [ Generics ] "(" [ ParamList ] ")" [ "->" Type ] ";"
           | "type" IDENT [ Generics ] ";"
           | "const" IDENT ":" Type ";"
```

The single seam to the embedding runtime. An `ext` item declares a binding the runtime provides: a function, an opaque type, or a constant. `ext` items have no bodies and produce no output — they exist so neoc can name-resolve and type-check calls into the host (`STATIC.md` §5, `DYNAMIC.md` §3.9).

This is the only mechanism by which a non-neoc binding becomes visible to neoc. There is no other.

## 4. Generics

```
Generics    ::= "<" GenericParam { "," GenericParam } [ "," ] ">"
GenericParam ::= IDENT [ ":" Bounds ]
Bounds      ::= TypePath { "+" TypePath }
WhereClause ::= "where" WherePred { "," WherePred } [ "," ]
WherePred   ::= IDENT ":" Bounds
```

Type parameters are declared in angle brackets and may carry trait bounds inline (`<T: Display>`) or in a trailing `where` clause. A `where` clause, when present, sits between the signature and the body:

```
FnDecl ::= "fn" IDENT [ Generics ] "(" [ ParamList ] ")" [ "->" Type ] [ WhereClause ] Block
```

Bounds use `:` and `+`, never `extends`. There are no lifetime parameters and no const generics.

## 5. Types

```
Type     ::= TypePath
           | "(" ")"                              (* unit type *)
           | "[" Type "]"                         (* list type *)
           | "fn" "(" [ TypeList ] ")" [ "->" Type ]   (* function type *)
           | "Self"
TypePath ::= IDENT [ TypeArgs ] { "::" IDENT [ TypeArgs ] }
TypeArgs ::= "<" Type { "," Type } [ "," ] ">"
```

- `TypePath` covers primitives (`number`, `string`, `bool`), named types, generic instantiations (`Result<number, ParseError>`), and qualified paths (`collections::Map<string, number>`).
- `()` is the unit type — the type of an expression evaluated for effect.
- `[T]` is the list type, constructed with list literals (§6) and indexed with `[]`.
- `fn(A, B) -> C` is a function type; the return clause is omitted for `-> ()`.

There are no reference types, no `dyn Trait`, no tuple types beyond unit, and no anonymous record/object types.

## 6. Expressions

```
Expr ::= Assignment
Assignment ::= OrExpr [ AssignOp Expr ]
AssignOp   ::= "=" | "+=" | "-=" | "*=" | "/=" | "%="

OrExpr   ::= AndExpr  { "||" AndExpr }
AndExpr  ::= CmpExpr  { "&&" CmpExpr }
CmpExpr  ::= RangeExpr [ ( "==" | "!=" | "<" | ">" | "<=" | ">=" ) RangeExpr ]
RangeExpr ::= AddExpr [ ( ".." | "..=" ) AddExpr ]
AddExpr  ::= MulExpr  { ( "+" | "-" ) MulExpr }
MulExpr  ::= Unary    { ( "*" | "/" | "%" ) Unary }
Unary    ::= ( "!" | "-" ) Unary | Postfix

Postfix  ::= Primary { PostfixOp }
PostfixOp ::= "." IDENT                          (* field access  *)
            | "." IDENT CallArgs                  (* method call   *)
            | "." INT_LITERAL                     (* newtype field *)
            | CallArgs                            (* function call *)
            | "[" Expr "]"                         (* index         *)
            | "?"                                  (* try           *)
CallArgs ::= "(" [ Expr { "," Expr } [ "," ] ] ")"

Primary  ::= Literal
           | PathExpr
           | StructLit
           | ListLit
           | "(" Expr ")"
           | "(" ")"                               (* unit value *)
           | BlockExpr
           | IfExpr
           | MatchExpr
           | Closure
           | "return" [ Expr ]
           | "break"
           | "continue"

Literal  ::= INT_LITERAL | FLOAT_LITERAL | STRING_LITERAL | BOOL_LITERAL
PathExpr ::= IDENT { "::" IDENT }
```

Precedence runs from lowest (assignment) to highest (postfix), as ordered above. Comparison and range are non-associative (no `a == b == c`, no `a..b..c`). `&&` and `||` short-circuit.

Operator notes:

- `+` is addition on `number` and concatenation on `string`. There is no `..` string-concatenation operator; `..` is exclusively the range operator.
- There is no ternary `? :`. Conditional values use `if` (§6.2). The postfix `?` is the try operator (§6.6); it never introduces a ternary.
- There is no `??`, no `?.`, no null coalescing — neoc has no null. Optionality is `Option<T>` (`STATIC.md` §3.6).

### 6.1 Block expressions

```
BlockExpr ::= "{" { Stmt } [ Expr ] "}"
```

A block is a sequence of statements followed by an optional final expression. The block's value is that final expression; a block with no final expression has type `()`. This is the Rust trailing-expression rule: a `let` or an `expr;` is a statement; an `expr` with no trailing `;` at the end of the block is the block's value.

### 6.2 `if`

```
IfExpr ::= "if" Expr Block [ "else" ( IfExpr | Block ) ]
```

`if` is an expression. The condition has no surrounding parentheses and must have type `bool`. When used for its value, both branches must be present and yield the same type; when used as a statement, the `else` may be omitted and the value is `()`.

### 6.3 `match`

```
MatchExpr ::= "match" Expr "{" { MatchArm } "}"
MatchArm  ::= Pattern [ "if" Expr ] "=>" Expr [ "," ]
```

`match` is an expression. It evaluates the scrutinee once and selects the first arm whose pattern matches and whose guard (if present) holds. All arms must yield the same type, and the arms must be exhaustive (`STATIC.md` §7).

### 6.4 Struct and enum literals

```
StructLit  ::= TypePath "{" [ FieldInitList ] "}"
FieldInitList ::= FieldInit { "," FieldInit } [ "," ] [ ".." Expr ]
FieldInit  ::= IDENT ":" Expr
             | IDENT                              (* shorthand: field == binding name *)
```

Named structs and struct-variant enums are constructed by listing fields: `Point { x: 1, y: 2 }`, `Shape::Circle { radius: 2.0 }`. Field shorthand (`Point { x, y }`) is allowed when a binding of the same name is in scope. A trailing `.. base` copies remaining fields from another value of the same type.

Unit structs and unit variants are constructed by naming them: `Marker`, `Color::Red`. Newtype structs and tuple variants are constructed by calling them: `UserId(7)`, `Shape::Move(1, 2)` — these use `CallArgs` (§6) and need no special production.

### 6.5 List literals

```
ListLit ::= "[" [ Expr { "," Expr } [ "," ] ] "]"
```

`[1, 2, 3]` constructs a value of type `[number]`. An empty `[]` requires a type annotation to infer its element type.

### 6.6 Closures

```
Closure ::= "|" [ ClosureParams ] "|" ClosureBody
ClosureParams ::= ClosureParam { "," ClosureParam } [ "," ]
ClosureParam  ::= IDENT [ ":" Type ]
ClosureBody   ::= Expr | Block
```

A closure is `|params| body`. Parameter types and the return type are usually inferred; annotate when inference is insufficient. `|x| x + 1` and `|x, y| { let s = x + y; s }` are both closures. There is no `move` keyword (no ownership to move) and no `=>` (that is the match-arm separator).

### 6.7 The `?` operator

`e?` is the postfix try operator (`PostfixOp`, §6). It short-circuits on the error/none case: if `e` is `Err(x)` it returns `Err(x)` from the enclosing function; if `Ok(v)` it evaluates to `v`. The same applies to `Option` (`None` returns `None`, `Some(v)` evaluates to `v`). It is valid only inside a function whose return type is the matching `Result` or `Option` (`STATIC.md` §6).

## 7. Patterns

```
Pattern     ::= LiteralPat
              | "_"                               (* wildcard *)
              | IDENT                             (* binding  *)
              | PathPat
              | StructPat
              | TuplePat
              | Pattern "|" Pattern               (* or-pattern *)
LiteralPat  ::= INT_LITERAL | FLOAT_LITERAL | STRING_LITERAL | BOOL_LITERAL
PathPat     ::= TypePath                          (* unit struct / unit variant *)
StructPat   ::= TypePath "{" [ FieldPatList ] "}"
FieldPatList ::= FieldPat { "," FieldPat } [ "," ] [ ".." ]
FieldPat    ::= IDENT ":" Pattern
              | IDENT                             (* shorthand binding *)
TuplePat    ::= TypePath "(" [ Pattern { "," Pattern } [ "," ] ] ")"
```

- A bare lowercase `IDENT` is a binding pattern; it binds the whole scrutinee.
- A `TypePath` (uppercase, possibly qualified) is a path pattern matching a unit struct or unit variant: `Color::Red`, `Marker`.
- `StructPat` matches named structs and struct variants, binding or testing fields: `Point { x, y }`, `Shape::Circle { radius }`. A trailing `..` ignores the rest.
- `TuplePat` matches newtype structs and tuple variants: `Some(x)`, `Ok(v)`, `Shape::Move(dx, dy)`.
- Or-patterns combine alternatives that bind the same variables: `1 | 2 | 3`.

## 8. Attributes

```
Attribute ::= "#" "[" AttrItem { "," AttrItem } "]"
AttrItem  ::= IDENT [ "(" [ AttrArg { "," AttrArg } ] ")" ]
AttrArg   ::= IDENT | Literal | AttrItem
```

Attributes attach metadata to the item or field that follows. Built-in attributes:

- `#[derive(Clone, Eq, Debug, Display)]` on a struct or enum — generates the named trait impls (`DYNAMIC.md` §6).
- `#[test]` on a zero-parameter `fn` — registers a test (`DYNAMIC.md` §6.5).
- Field validation attributes (e.g. `#[min(0)]`, `#[max_len(200)]`) on a struct field — generate construction-time checks.

There is no general user-defined macro system in the first edition; the attribute set is fixed.

## 9. Doc comments as attributes

A `///` line comment and a `/** … */` block comment (`LEXICAL.md` §3) attached to an item are sugar for `#[doc = "…"]`. They are parsed as attributes and carried through to tooling. This lets §2 treat documentation uniformly with other attributes.

## 10. Complete grammar

Every production from §1–§9, collected in dependency order. Implementers parse against this section; §1–§9 are commentary. Token classes (`UPPER_SNAKE_CASE`) and keyword/punctuation literals resolve to `LEXICAL.md` §8. Cross-checked against the tree-sitter grammar in `crates/neoc-grammar/grammar.js`; where the two disagree, this section wins.

```
(* source file *)
SourceFile  ::= { Item }

(* items *)
Item        ::= { Attribute } [ "pub" ] ItemKind
ItemKind    ::= UseDecl
              | ConstDecl
              | TypeAlias
              | StructDecl
              | EnumDecl
              | TraitDecl
              | ImplBlock
              | FnDecl
              | ExtBlock
              | ExternBlock

(* declarations *)
UseDecl     ::= "use" UsePath [ "as" IDENT ] ";"
UsePath     ::= IDENT { "::" IDENT }

ConstDecl   ::= "const" IDENT ":" Type "=" Expr ";"

TypeAlias   ::= "type" IDENT [ Generics ] "=" Type ";"

StructDecl  ::= "struct" IDENT [ Generics ] StructTail
StructTail  ::= ";"
              | "(" Type ")" ";"
              | FieldDeclList
FieldDeclList ::= "{" [ FieldDecl { "," FieldDecl } [ "," ] ] "}"
FieldDecl   ::= { Attribute } IDENT ":" Type

EnumDecl    ::= "enum" IDENT [ Generics ] "{" [ Variant { "," Variant } [ "," ] ] "}"
Variant     ::= { Attribute } IDENT [ VariantTail ]
VariantTail ::= "(" [ Type { "," Type } [ "," ] ] ")"
              | FieldDeclList

TraitDecl   ::= "trait" IDENT [ Generics ] "{" { TraitItem } "}"
TraitItem   ::= FnSig ";"
              | FnSig Block
FnSig       ::= "fn" IDENT [ Generics ] ParamList [ "->" Type ] [ WhereClause ]

ImplBlock   ::= "impl" [ Generics ] ImplHead "{" { ImplItem } "}"
ImplHead    ::= TypePath "for" Type
              | Type
ImplItem    ::= { Attribute } FnDecl

FnDecl      ::= FnSig Block
ParamList   ::= "(" [ ( SelfParam | Param ) { "," Param } [ "," ] ] ")"
SelfParam   ::= "self"
Param       ::= IDENT ":" Type

ExtBlock    ::= "ext" "{" { ExtItem } "}"
              | "ext" ExtItem
ExternBlock ::= "extern" STRING_LITERAL "{" { ExtItem } "}"
ExtItem     ::= FnSig ";"
              | "type" IDENT [ Generics ] ";"
              | "const" IDENT ":" Type ";"

(* generics *)
Generics    ::= "<" GenericParam { "," GenericParam } [ "," ] ">"
GenericParam ::= IDENT [ ":" Bounds ]
Bounds      ::= TypePath { "+" TypePath }
WhereClause ::= "where" WherePred { "," WherePred } [ "," ]
WherePred   ::= IDENT ":" Bounds

(* types *)
Type        ::= "(" ")"
              | "[" Type "]"
              | "fn" "(" [ Type { "," Type } [ "," ] ] ")" [ "->" Type ]
              | "Self"
              | TypePath
TypePath    ::= IDENT [ TypeArgs ] { "::" IDENT [ TypeArgs ] }
TypeArgs    ::= "<" Type { "," Type } [ "," ] ">"

(* expressions — precedence runs lowest (assignment) to highest (postfix) *)
Expr        ::= Assignment
Assignment  ::= OrExpr [ AssignOp Expr ]
AssignOp    ::= "=" | "+=" | "-=" | "*=" | "/=" | "%="
OrExpr      ::= AndExpr { "||" AndExpr }
AndExpr     ::= CmpExpr { "&&" CmpExpr }
CmpExpr     ::= RangeExpr [ ( "==" | "!=" | "<" | ">" | "<=" | ">=" ) RangeExpr ]
RangeExpr   ::= AddExpr [ ( ".." | "..=" ) AddExpr ]
AddExpr     ::= MulExpr { ( "+" | "-" ) MulExpr }
MulExpr     ::= Unary { ( "*" | "/" | "%" ) Unary }
Unary       ::= ( "!" | "-" ) Unary | Postfix
Postfix     ::= Primary { PostfixOp }
PostfixOp   ::= "." IDENT
              | "." IDENT CallArgs
              | "." INT_LITERAL
              | CallArgs
              | "[" Expr "]"
              | "?"
CallArgs    ::= "(" [ Expr { "," Expr } [ "," ] ] ")"

Primary     ::= Literal
              | PathExpr
              | StructLit
              | ListLit
              | "(" Expr ")"
              | "(" ")"
              | Block
              | IfExpr
              | MatchExpr
              | WhileExpr
              | ForExpr
              | LoopExpr
              | Closure
              | "return" [ Expr ]
              | "break"
              | "continue"

Literal     ::= INT_LITERAL | FLOAT_LITERAL | STRING_LITERAL | BOOL_LITERAL
PathExpr    ::= IDENT { "::" IDENT }

Block       ::= "{" { Stmt } [ Expr ] "}"

IfExpr      ::= "if" CondExpr Block [ "else" ( IfExpr | Block ) ]
MatchExpr   ::= "match" CondExpr "{" [ MatchArm { "," MatchArm } [ "," ] ] "}"
MatchArm    ::= Pattern [ "if" Expr ] "=>" Expr
WhileExpr   ::= "while" CondExpr Block
ForExpr     ::= "for" Pattern "in" CondExpr Block
LoopExpr    ::= "loop" Block

StructLit   ::= PathExpr "{" [ FieldInit { "," FieldInit } [ "," ] ] [ ".." Expr ] "}"
FieldInit   ::= IDENT ":" Expr
              | IDENT
ListLit     ::= "[" [ Expr { "," Expr } [ "," ] ] "]"

Closure     ::= ClosureParams ( Expr | Block )
ClosureParams ::= "|" [ ClosureParam { "," ClosureParam } [ "," ] ] "|"
ClosureParam ::= IDENT [ ":" Type ]

(* CondExpr is the head of `if` / `while` / `for ... in` / `match`. It excludes
   the bare struct literal so the trailing `{` is unambiguously the block / arm
   list; to use a struct literal there, parenthesize it. Otherwise it is Expr
   without an assignment at the top level. *)
CondExpr    ::= Expr  (* minus StructLit and top-level Assignment; see note above *)

(* patterns *)
Pattern     ::= PatternNoOr { "|" PatternNoOr }
PatternNoOr ::= "_"
              | LiteralPat
              | StructPat
              | TuplePat
              | PathPat
              | IDENT                     (* binding *)
LiteralPat  ::= INT_LITERAL | FLOAT_LITERAL | STRING_LITERAL | BOOL_LITERAL
PathPat     ::= IDENT "::" IDENT { "::" IDENT }
PatternPath ::= IDENT { "::" IDENT }
StructPat   ::= PatternPath "{" [ FieldPat { "," FieldPat } [ "," ] ] [ ".." ] "}"
FieldPat    ::= IDENT ":" Pattern
              | IDENT
TuplePat    ::= PatternPath "(" [ Pattern { "," Pattern } [ "," ] ] ")"

(* statements *)
Stmt        ::= LetStmt
              | Item
              | BlockStmt
              | ExprStmt
LetStmt     ::= "let" [ "mut" ] Pattern [ ":" Type ] [ "=" Expr ] ";"
BlockStmt   ::= ( IfExpr | MatchExpr | WhileExpr | ForExpr | LoopExpr | Block ) [ ";" ]
ExprStmt    ::= Expr ";"

(* attributes *)
Attribute   ::= "#" "[" AttrItem { "," AttrItem } [ "," ] "]"
AttrItem    ::= IDENT [ "(" [ AttrArg { "," AttrArg } [ "," ] ] ")" ]
AttrArg     ::= IDENT "=" Literal
              | AttrItem
              | Literal
```

Notes on grammar-faithful additions beyond §1–§9:

- `loop { … }` (`LoopExpr`), `while`, and `for … in` are expression and statement forms the tree-sitter grammar accepts; §6 documents `if` and `match` as the primary value-producing forms.
- `extern "<host>" { … }` (`ExternBlock`) is a hosted variant of `ext` whose string literal names the host runtime (mirroring `extern "C"`); it reuses `ExtItem`.
- `let mut x = …` is accepted; `mut` is a reserved keyword (`LEXICAL.md` §5).
- A `PathPat` is two or more `::`-separated segments (a unit variant such as `Color::Red`); a single segment is a binding pattern that name resolution may reinterpret as a unit struct/variant — exactly Rust's `None` handling (`STATIC.md` §2).
- `CondExpr` is the one place the expression grammar is restricted: a bare struct literal is excluded so `if Foo { … }` reads as `if` with a block, never as a struct literal. Parenthesize to use one (`(Foo { x: 1 }).x`).
