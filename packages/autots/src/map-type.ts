const NEOC_KEYWORDS = new Set([
  "fn", "let", "const", "mut", "struct", "enum", "trait", "impl", "ext", "extern",
  "use", "pub", "as", "if", "else", "match", "while", "for", "loop", "in", "return",
  "break", "continue", "type", "self", "true", "false",
]);

/** Whether `name` is a legal neoc identifier (and not a keyword). JS allows `$`/keywords; neoc does not. */
export function isNeocIdent(name: string): boolean {
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(name) && !NEOC_KEYWORDS.has(name);
}
