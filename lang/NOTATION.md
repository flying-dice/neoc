# Notation

This file defines the meta-notations the rest of `lang/` uses. Every other layer file refers back here rather than restating conventions.

Three notations cover the four layers:

| Layer | File | Notation |
| --- | --- | --- |
| Lexical | `LEXICAL.md` | EBNF (token rules) + prose |
| Syntax | `SYNTAX.md` | EBNF (grammar productions) |
| Static semantics | `STATIC.md` | Inference rules + prose |
| Dynamic semantics | `DYNAMIC.md` | Lowering brackets + prose |

---

## EBNF

`LEXICAL.md` and `SYNTAX.md` use a single EBNF dialect, adapted from ISO/IEC 14977 for readability. Lua's Reference Manual §9 is the closest existing example.

| Form | Meaning |
| --- | --- |
| `Name ::= production` | A production rule. `Name` is a non-terminal. |
| `"literal"` | A literal token, matched character-for-character. |
| `'literal'` | Same as `"literal"`; single quotes used when the literal contains `"`. |
| `A B` | Sequence: `A` followed by `B`. |
| `A \| B` | Alternation: `A` or `B`. |
| `[ A ]` | Optional: `A` appears zero or one times. |
| `{ A }` | Repetition: `A` appears zero or more times. |
| `{ A }+` | Repetition: `A` appears one or more times. |
| `( A )` | Grouping. |
| `A - B` | Subtraction: any `A` that is not also a `B`. (Used in lexical productions, e.g. identifiers minus keywords.) |
| `(* comment *)` | A comment within the EBNF. Not part of the matched syntax. |

Conventions:

- **Non-terminals** are `UpperCamelCase` (`Expr`, `MatchArm`, `StructDecl`).
- **Token classes** defined in `LEXICAL.md` are `UPPER_SNAKE_CASE` (`IDENT`, `INT_LITERAL`, `STRING_LITERAL`).
- **Keyword and punctuation literals** are written as the literal source (`"struct"`, `"->"`, `"::"`).
- Productions are read top-down. Whitespace and comments are removed by the lexer; the syntax productions assume the token stream, not raw source.

---

## Inference rules

`STATIC.md` uses natural-deduction inference rules. The Standard ML Definition (Milner, Tofte, Harper) is the canonical example of this style.

A rule has the form:

```
premise₁    premise₂    …    premiseₙ
─────────────────────────────────────── (RULE-NAME)
              conclusion
```

Read as: if every premise above the line holds, then the conclusion below the line holds.

Common judgements:

| Form | Meaning |
| --- | --- |
| `Γ ⊢ e : τ` | Under environment `Γ`, expression `e` has type `τ`. |
| `Γ ⊢ p : τ ⇒ Γ'` | Under `Γ`, pattern `p` matches a value of type `τ` and extends the environment to `Γ'`. |
| `Γ ⊢ d ⇒ Γ'` | Under `Γ`, declaration `d` produces an extended environment `Γ'`. |
| `Γ ⊢ τ ok` | Under `Γ`, type `τ` is well-formed. |

Conventions:

- `Γ` (capital gamma) is the type environment — a finite map from identifiers to types.
- `Γ, x : τ` is `Γ` extended with the binding `x : τ`. The new binding shadows any prior `x`.
- `τ`, `σ` are metavariables ranging over types.
- `e`, `e₁`, `e₂` range over expressions; `p` over patterns; `d` over declarations.
- Rule names are `UPPER-KEBAB-CASE` in parentheses to the right of the bar.

Rules that always hold without premises (axioms) have an empty top:

```
─────────────────── (T-TRUE)
Γ ⊢ true : bool
```

---

## Lowering brackets

`DYNAMIC.md` defines neoc's dynamic semantics as a translation to Lua 5.1 source. The notation:

```
⟦ neoc-construct ⟧ = lua-source
```

Read as: the neoc construct on the left compiles to the Lua source on the right. The brackets `⟦ ⟧` (mathematical white square brackets, U+27E6 / U+27E7) wrap a piece of neoc; the right-hand side is a literal Lua snippet, with metavariables for sub-pieces.

Sub-piece lowering is recursive:

```
⟦ let x = e ⟧ = local x = ⟦ e ⟧
```

The `⟦ e ⟧` on the right means "the lowering of the neoc expression `e`."

Conventions:

- Lua snippets on the right are written verbatim — exact whitespace, indentation, and operators.
- When a single neoc construct lowers to multiple Lua lines, indent and present as a code block.
- Metavariables in the Lua text are written in `⟪ ⟫` (white parenthesis brackets) to distinguish them from literal Lua: `local ⟪name⟫ = ⟦ ⟪init⟫ ⟧`.
- Constructs whose lowering depends on context (e.g. tail position, expression vs. statement) are annotated with subscripts: `⟦ e ⟧ₛ` for statement context, `⟦ e ⟧ₑ` for expression context.

Worked examples appear with both forms side by side:

```
⟦ if cond { then-block } else { else-block } ⟧ₑ =
  (function()
    if ⟦ cond ⟧ then ⟦ then-block ⟧ else ⟦ else-block ⟧ end
  end)()
```

---

## ASCII fallback

Most files prefer the Unicode forms (`Γ`, `τ`, `⟦ ⟧`, `→`). When a tool or terminal cannot render them, the following ASCII fallbacks are accepted in PR comments and conformance test names but **never** in the spec files themselves:

| Unicode | ASCII |
| --- | --- |
| `Γ` | `G` |
| `⊢` | `\|-` |
| `→` | `->` |
| `⟦ e ⟧` | `[[ e ]]` |
| `τ` | `T` |
| `⇒` | `==>` |

The spec is the canonical Unicode form. ASCII is for talk *about* the spec.
