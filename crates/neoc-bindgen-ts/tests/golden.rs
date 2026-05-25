//! Golden test: the `color.d.ts` fixture must transpile to the committed
//! `color.neoc`. Exercises clean primitive mapping, an opaque declared handle
//! (`Rgb`), and the `JsValue` fallback with a `// was:` comment (`merge<T>`).

use neoc_bindgen_ts::ts_to_neoc;

#[test]
fn color_fixture_matches_golden() {
    let source = include_str!("fixtures/color.d.ts");
    let expected = include_str!("fixtures/color.neoc");
    let actual = ts_to_neoc(source, "color").expect("transpile color.d.ts");
    assert_eq!(actual, expected);
}
