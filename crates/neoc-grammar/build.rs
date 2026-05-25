fn main() {
    // The grammar is the `tree-sitter-neoc` submodule (the canonical source);
    // we compile its committed parser.
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
