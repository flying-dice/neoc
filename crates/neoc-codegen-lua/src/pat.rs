//! Pattern lowering — the test/bind sequences a `match` arm compiles to.
//!
//! Each pattern compiles against a *subject*: a Lua expression naming the value
//! it inspects (the scrutinee `__s`, or a projection like `__s.x` / `__s[1]`).
//! [`Emitter::compile_pattern`] returns a boolean **test** over the subject and
//! the **bindings** it introduces, both as Lua source. `match` (in [`expr`])
//! stitches arms together; exhaustiveness is guaranteed upstream, so no
//! fallback arm is emitted.

use crate::Emitter;
use crate::expr::VariantShape;
use neoc_ast as ast;

/// A pattern's compiled test and the locals it binds when the test passes.
#[derive(Default)]
pub(crate) struct Compiled {
    /// Boolean Lua expressions, `and`-joined to form the arm's test.
    pub tests: Vec<String>,
    /// `local x = <subject>` lines, emitted after the test passes.
    pub binds: Vec<String>,
}

impl Compiled {
    /// The combined test, or `"true"` when the pattern is irrefutable.
    pub(crate) fn test(&self) -> String {
        if self.tests.is_empty() {
            "true".to_string()
        } else {
            self.tests.join(" and ")
        }
    }
}

impl Emitter {
    /// Compile `pattern` against `subject` (a Lua expression for the value
    /// being matched), accumulating tests and bindings.
    pub(crate) fn compile_pattern(&mut self, pattern: &ast::Pattern, subject: &str) -> Compiled {
        let mut c = Compiled::default();
        self.compile_into(pattern, subject, &mut c);
        c
    }

    fn compile_into(&mut self, pattern: &ast::Pattern, subject: &str, c: &mut Compiled) {
        match pattern {
            ast::Pattern::Wildcard => {}
            ast::Pattern::Binding { name, .. } => {
                // A bare name that resolves to a unit variant (e.g. `None`) is
                // a constant pattern, not a fresh binding — test the tag.
                if let Some((en, var, VariantShape::Unit)) =
                    self.pattern_variant(std::slice::from_ref(name))
                {
                    self.note_prelude(&en);
                    c.tests.push(format!("{subject}.tag == \"{var}\""));
                } else {
                    c.binds.push(format!("local {name} = {subject}"));
                }
            }
            ast::Pattern::Literal(lit) => {
                c.tests.push(format!("{subject} == {}", lua_literal(lit)));
            }
            ast::Pattern::Path(segments) => self.compile_path(segments, subject, c),
            ast::Pattern::Struct {
                path,
                fields,
                rest: _,
            } => self.compile_struct(path, fields, subject, c),
            ast::Pattern::Tuple { path, elems } => self.compile_tuple(path, elems, subject, c),
            ast::Pattern::Or(alts) => self.compile_or(alts, subject, c),
        }
    }

    /// A path pattern: a unit variant (`Color::Red`) tests the tag; a lone
    /// path that resolves to nothing is treated as a binding.
    fn compile_path(&mut self, segments: &[ast::Ident], subject: &str, c: &mut Compiled) {
        if let Some((en, var, _)) = self.pattern_variant(segments) {
            self.note_prelude(&en);
            c.tests.push(format!("{subject}.tag == \"{var}\""));
        } else if let [name] = segments {
            c.binds.push(format!("local {name} = {subject}"));
        } else {
            self.error(format!(
                "pattern path `{}` is not a known enum variant",
                segments.join("::")
            ));
        }
    }

    /// A struct pattern: either a named struct (`Point { x, y }`, tested by
    /// `__struct`) or a struct-shaped enum variant (`Shape::Circle { r }`,
    /// tested by `tag`). Each field recurses against `subject.field`.
    fn compile_struct(
        &mut self,
        path: &[ast::Ident],
        fields: &[ast::FieldPat],
        subject: &str,
        c: &mut Compiled,
    ) {
        if let Some((en, var, VariantShape::Named)) = self.pattern_variant(path) {
            self.note_prelude(&en);
            c.tests.push(format!("{subject}.tag == \"{var}\""));
        } else if let Some(name) = path.last() {
            c.tests.push(format!("{subject}.__struct == \"{name}\""));
        }
        for f in fields {
            let field_subject = format!("{subject}.{}", f.name);
            match &f.pattern {
                Some(p) => self.compile_into(p, &field_subject, c),
                // Shorthand `{ x }` binds the field to its own name.
                None => c.binds.push(format!("local {} = {field_subject}", f.name)),
            }
        }
    }

    /// A tuple-variant pattern (`Shape::Move(x, y)`): test the tag, then bind
    /// each element from the positional payload `subject[1]`, `subject[2]`, ….
    fn compile_tuple(
        &mut self,
        path: &[ast::Ident],
        elems: &[ast::Pattern],
        subject: &str,
        c: &mut Compiled,
    ) {
        if let Some((en, var, _)) = self.pattern_variant(path) {
            self.note_prelude(&en);
            c.tests.push(format!("{subject}.tag == \"{var}\""));
        } else if let Some(name) = path.last() {
            // A newtype struct pattern destructures the `[1]` payload.
            c.tests.push(format!("{subject}.__struct == \"{name}\""));
        }
        for (i, elem) in elems.iter().enumerate() {
            let elem_subject = format!("{subject}[{}]", i + 1);
            self.compile_into(elem, &elem_subject, c);
        }
    }

    /// An or-pattern (`A | B`): the test is the disjunction of the alternatives'
    /// tests. Alternatives bind the same names (checked upstream), so the first
    /// alternative's bindings are emitted.
    fn compile_or(&mut self, alts: &[ast::Pattern], subject: &str, c: &mut Compiled) {
        let mut tests = Vec::new();
        let mut first_binds = None;
        for alt in alts {
            let compiled = self.compile_pattern(alt, subject);
            tests.push(format!("({})", compiled.test()));
            if first_binds.is_none() {
                first_binds = Some(compiled.binds);
            }
        }
        if !tests.is_empty() {
            c.tests.push(format!("({})", tests.join(" or ")));
        }
        if let Some(binds) = first_binds {
            c.binds.extend(binds);
        }
    }
}

/// Render a literal pattern as the Lua value it compares against.
fn lua_literal(lit: &ast::LiteralPat) -> String {
    match lit {
        ast::LiteralPat::Int(s) | ast::LiteralPat::Float(s) => s.replace('_', ""),
        ast::LiteralPat::Str(s) => crate::expr::lua_string(s),
        ast::LiteralPat::Bool(b) => b.to_string(),
    }
}
