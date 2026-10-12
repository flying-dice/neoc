fn main() {
    // `tree-sitter-neoc/grammar.js` is the grammar source; `tree-sitter generate`
    // produces the committed `src/parser.c`, which we compile.
    let src = std::path::Path::new("tree-sitter-neoc").join("src");
    cc::Build::new()
        .include(&src)
        .file(src.join("parser.c"))
        .flag_if_supported("-Wno-unused-parameter")
        .warnings(false)
        .compile("tree-sitter-neoc");

    println!("cargo:rerun-if-changed=tree-sitter-neoc/src/parser.c");
    println!("cargo:rerun-if-changed=tree-sitter-neoc/grammar.js");
}
