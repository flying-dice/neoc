//! Pattern lowering — a neoc pattern compiled to a Python `case` pattern.
//!
//! Python's structural pattern matching is the native target: a unit variant is
//! `case None_()`, a tuple variant `case Some(v)` (positional capture), a
//! struct/struct-variant `case Point(x=x, y=y)` (keyword capture), a literal
//! `case 42`, a wildcard `case _`, a binding `case name`, and an or-pattern
//! `case A() | B()`. Exhaustiveness is guaranteed upstream, so no fallback case
//! is needed.

use crate::Emitter;
use crate::VariantShape;
use neoc_ast as ast;

impl Emitter {
    /// Compile `pattern` to a Python `case` pattern string (everything between
    /// `case ` and the `:`).
    pub(crate) fn compile_case(&mut self, pattern: &ast::Pattern) -> String {
        match pattern {
            ast::Pattern::Wildcard => "_".to_string(),
            ast::Pattern::Binding { name, .. } => {
                // A bare name resolving to a unit variant (e.g. `None`) is a
                // constant pattern; otherwise it is a capture.
                if let Some((en, var, VariantShape::Unit)) =
                    self.pattern_variant(std::slice::from_ref(name))
                {
                    self.note_prelude(&en);
                    format!("{}()", crate::variant_class_name(&var))
                } else {
                    name.clone()
                }
            }
            ast::Pattern::Literal(lit) => py_literal(lit),
            ast::Pattern::Path(segments) => self.compile_path(segments),
            ast::Pattern::Struct { path, fields, .. } => self.compile_struct(path, fields),
            ast::Pattern::Tuple { path, elems } => self.compile_tuple(path, elems),
            ast::Pattern::Or(alts) => {
                let parts: Vec<String> = alts.iter().map(|a| self.compile_case(a)).collect();
                parts.join(" | ")
            }
        }
    }

    /// A path pattern: a unit variant (`Color::Red` / `None`) is a constant
    /// class pattern; a lone unresolved path is a capture.
    fn compile_path(&mut self, segments: &[ast::Ident]) -> String {
        if let Some((en, var, _)) = self.pattern_variant(segments) {
            self.note_prelude(&en);
            format!("{}()", crate::variant_class_name(&var))
        } else if let [name] = segments {
            name.clone()
        } else {
            self.error(format!(
                "pattern path `{}` is not a known enum variant",
                segments.join("::")
            ));
            "_".to_string()
        }
    }

    /// A struct / struct-variant pattern: keyword capture against the dataclass
    /// (`Point(x=x, y=y)`, `Shape.Circle` → `Circle(r=r)`).
    fn compile_struct(&mut self, path: &[ast::Ident], fields: &[ast::FieldPat]) -> String {
        let cls = if let Some((en, var, _)) = self.pattern_variant(path) {
            self.note_prelude(&en);
            crate::variant_class_name(&var)
        } else {
            path.last().cloned().unwrap_or_default()
        };
        let parts: Vec<String> = fields
            .iter()
            .map(|f| {
                let sub = match &f.pattern {
                    Some(p) => self.compile_case(p),
                    // Shorthand `{ x }` captures the field into its own name.
                    None => f.name.clone(),
                };
                format!("{}={sub}", f.name)
            })
            .collect();
        format!("{cls}({})", parts.join(", "))
    }

    /// A tuple-variant / newtype pattern: positional capture against the
    /// dataclass (`Some(v)`, `Move(x, y)`). The dataclass stores the payload as
    /// `_0`, `_1`, …, so positional `case` patterns match field order.
    fn compile_tuple(&mut self, path: &[ast::Ident], elems: &[ast::Pattern]) -> String {
        let cls = if let Some((en, var, _)) = self.pattern_variant(path) {
            self.note_prelude(&en);
            crate::variant_class_name(&var)
        } else {
            path.last().cloned().unwrap_or_default()
        };
        let parts: Vec<String> = elems.iter().map(|p| self.compile_case(p)).collect();
        format!("{cls}({})", parts.join(", "))
    }
}

/// Render a literal pattern as the Python value it compares against.
fn py_literal(lit: &ast::LiteralPat) -> String {
    match lit {
        ast::LiteralPat::Int(s) | ast::LiteralPat::Float(s) => s.replace('_', ""),
        ast::LiteralPat::Str(s) => crate::expr::py_string(s),
        ast::LiteralPat::Bool(b) => if *b { "True" } else { "False" }.to_string(),
    }
}
