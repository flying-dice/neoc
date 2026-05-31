//! Semantic tokens: classify the document's tokens for type-aware highlighting.
//!
//! A focused, high-confidence classifier over the CST — keywords, comments,
//! number and string literals, function and type names, parameters, fields, and
//! enum variants. Other identifiers fall back to `variable`. Multi-line tokens
//! (a block comment spanning lines) are skipped, since the LSP encoding is
//! per-line. Classified tokens are exposed decoded for testing and delta-encoded
//! into the wire format by [`encode`].

use tower_lsp::lsp_types::{SemanticToken, SemanticTokenType};
use tree_sitter::Node;

use crate::cst;
use crate::position::byte_to_position;

/// The token types neoc classifies, in legend order — a token's index here is
/// its `tokenType` in the encoded data and in the server's advertised legend.
pub const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::KEYWORD,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::TYPE,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::NUMBER,
    SemanticTokenType::STRING,
    SemanticTokenType::COMMENT,
];

// Indices into TOKEN_TYPES.
const KEYWORD: u32 = 0;
const FUNCTION: u32 = 1;
const TYPE: u32 = 2;
const PARAMETER: u32 = 3;
const VARIABLE: u32 = 4;
const PROPERTY: u32 = 5;
const ENUM_MEMBER: u32 = 6;
const NUMBER: u32 = 7;
const STRING: u32 = 8;
const COMMENT: u32 = 9;

/// neoc keywords, recognised among the grammar's anonymous tokens.
const KEYWORDS: &[&str] = &[
    "fn", "let", "const", "struct", "enum", "trait", "impl", "type", "use", "match", "if", "else",
    "for", "while", "loop", "break", "continue", "return", "self", "ext", "pub", "as", "in",
    "where",
];

/// A classified token: 0-based line and UTF-16 column, UTF-16 length, and the
/// index of its type in [`TOKEN_TYPES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemToken {
    pub line: u32,
    pub start: u32,
    pub len: u32,
    pub type_index: u32,
}

/// The classified tokens of `source`, in document order.
#[must_use]
pub fn semantic_tokens(source: &str) -> Vec<SemToken> {
    let tree = cst::parse(source);
    let mut out = Vec::new();
    collect(tree.root_node(), source, &mut out);
    out.sort_by_key(|t| (t.line, t.start));
    out
}

/// Delta-encodes classified `tokens` into the LSP wire form: each token's line
/// and start are relative to the previous token (the same line resets `start`
/// relative to it; a new line gives the absolute column).
#[must_use]
pub fn encode(tokens: &[SemToken]) -> Vec<SemanticToken> {
    let mut data = Vec::with_capacity(tokens.len());
    let (mut prev_line, mut prev_start) = (0, 0);
    for token in tokens {
        let delta_line = token.line - prev_line;
        let delta_start = if delta_line == 0 {
            token.start - prev_start
        } else {
            token.start
        };
        data.push(SemanticToken {
            delta_line,
            delta_start,
            length: token.len,
            token_type: token.type_index,
            token_modifiers_bitset: 0,
        });
        prev_line = token.line;
        prev_start = token.start;
    }
    data
}

fn collect(node: Node, source: &str, out: &mut Vec<SemToken>) {
    if let Some(type_index) = classify(node, source) {
        push(node, source, type_index, out);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, source, out);
    }
}

/// The token type for `node`, or `None` when it is not a token neoc colours.
fn classify(node: Node, source: &str) -> Option<u32> {
    match node.kind() {
        "line_comment" | "block_comment" | "doc_comment" => Some(COMMENT),
        "integer_literal" | "float_literal" => Some(NUMBER),
        "boolean_literal" => Some(KEYWORD),
        "string_literal" => Some(STRING),
        "identifier" => Some(classify_identifier(node, source)),
        kind if KEYWORDS.contains(&kind) => Some(KEYWORD),
        _ => None,
    }
}

/// Classifies an `identifier` by the role its parent gives it, defaulting to a
/// variable when no specific role applies.
fn classify_identifier(node: Node, _source: &str) -> u32 {
    let Some(parent) = node.parent() else {
        return VARIABLE;
    };
    let is_named_field = parent
        .child_by_field_name("name")
        .is_some_and(|n| n.id() == node.id());
    match parent.kind() {
        "function_declaration" if is_named_field => FUNCTION,
        "struct_declaration" | "enum_declaration" | "trait_declaration" | "type_alias"
            if is_named_field =>
        {
            TYPE
        }
        "parameter" if is_named_field => PARAMETER,
        "field_declaration" if is_named_field => PROPERTY,
        "variant" if is_named_field => ENUM_MEMBER,
        "type_path" => TYPE,
        _ if is_call_callee(node) => FUNCTION,
        _ => VARIABLE,
    }
}

/// Whether `node` is the callee of a call — the identifier itself, or the head
/// of a `path`, sitting in a `call_expression`'s `function` field.
fn is_call_callee(node: Node) -> bool {
    let candidate = match node.parent() {
        Some(parent) if parent.kind() == "path" => parent,
        _ => node,
    };
    candidate.parent().is_some_and(|call| {
        call.kind() == "call_expression"
            && call
                .child_by_field_name("function")
                .is_some_and(|f| f.id() == candidate.id())
    })
}

/// Pushes a single-line token for `node`; multi-line nodes are skipped, since
/// the LSP encoding is per-line.
fn push(node: Node, source: &str, type_index: u32, out: &mut Vec<SemToken>) {
    let start = byte_to_position(source, node.start_byte());
    let end = byte_to_position(source, node.end_byte());
    if start.line != end.line {
        return;
    }
    out.push(SemToken {
        line: start.line,
        start: start.character,
        len: end.character - start.character,
        type_index,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(line, start, len, type-name)` for each token, 0-based.
    fn typed(src: &str) -> Vec<(u32, u32, u32, &'static str)> {
        semantic_tokens(src)
            .into_iter()
            .map(|t| {
                (
                    t.line,
                    t.start,
                    t.len,
                    TOKEN_TYPES[t.type_index as usize].as_str(),
                )
            })
            .collect()
    }

    #[test]
    fn classifies_a_function_declaration() {
        let src = "fn inc(n: number) -> number { n }\n";
        let tokens = typed(src);
        // `fn` keyword, `inc` function, `n` parameter, `number` type ×2, `n` var.
        assert!(tokens.contains(&(0, 0, 2, "keyword")), "{tokens:?}");
        assert!(tokens.contains(&(0, 3, 3, "function")), "{tokens:?}");
        assert!(tokens.contains(&(0, 7, 1, "parameter")), "{tokens:?}");
        assert!(tokens.iter().any(|t| t.3 == "type"), "{tokens:?}");
    }

    #[test]
    fn classifies_literals_and_comments() {
        let src = "// note\nconst N: number = 42\nconst S: string = \"hi\"\n";
        let tokens = typed(src);
        assert!(tokens.contains(&(0, 0, 7, "comment")), "{tokens:?}");
        assert!(tokens.iter().any(|t| t.3 == "number"), "{tokens:?}");
        assert!(tokens.iter().any(|t| t.3 == "string"), "{tokens:?}");
    }

    #[test]
    fn encode_delta_compresses_positions() {
        // Two tokens on the same line: second is delta-encoded from the first.
        let tokens = vec![
            SemToken {
                line: 0,
                start: 0,
                len: 2,
                type_index: KEYWORD,
            },
            SemToken {
                line: 0,
                start: 3,
                len: 3,
                type_index: FUNCTION,
            },
        ];
        assert_eq!(
            encode(&tokens),
            vec![
                SemanticToken {
                    delta_line: 0,
                    delta_start: 0,
                    length: 2,
                    token_type: KEYWORD,
                    token_modifiers_bitset: 0,
                },
                SemanticToken {
                    delta_line: 0,
                    delta_start: 3,
                    length: 3,
                    token_type: FUNCTION,
                    token_modifiers_bitset: 0,
                },
            ]
        );
    }

    #[test]
    fn call_callee_is_a_function() {
        let src = "fn main() -> number { inc(1) }\n";
        let tokens = typed(src);
        // `inc` at the call site is classified as a function.
        assert!(
            tokens.iter().any(|t| t.3 == "function" && t.1 == 22),
            "{tokens:?}"
        );
    }
}
