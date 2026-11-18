#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The shipped Calculator (`apps/calculator`, not a fixture) under the real runtime: its arithmetic, its
//! number formatting through the tools, and what its tools declare to agents.

mod common;

use agentos_application::ports::{
    AppData, AppRuntime, CallContext, Notifier, PortResult, RenderOutput, ViewEventCall,
};
use agentos_domain::{ui::Node, Capabilities};
use agentos_luau::{Limits, LuauRuntime};
use common::{agent_ctx, built_app, db, id, user_ctx};
use serde_json::{json, Map, Value};
use std::{path::Path, sync::Arc};

struct Quiet;
#[async_trait::async_trait]
impl Notifier for Quiet {
    async fn notify(&self, _: agentos_domain::Notification) -> PortResult<()> {
        Ok(())
    }
}

async fn calculator() -> LuauRuntime {
    let package = built_app("calculator");
    let runtime = LuauRuntime::new(
        Arc::new(Quiet),
        Arc::new(agentos_application::testing::RecordingShell::default()),
        Limits::default(),
    );
    let data: Arc<dyn AppData> = db();
    runtime.load(&id("calculator"), package, data, Capabilities::all()).await.unwrap();
    runtime
}

async fn tool(rt: &LuauRuntime, name: &str, args: Value, ctx: CallContext) -> PortResult<Value> {
    rt.call_tool(&id("calculator"), name, args, ctx, false).await
}

#[tokio::test]
async fn calculate_evaluates_exactly_and_rejects_what_is_not_arithmetic() {
    let rt = calculator().await;
    for (expression, result) in [
        ("1 + 2 * 3", 7.0),
        ("(1 + 2) * 3", 9.0),
        ("2 ^ 3 ^ 2", 512.0),
        ("-4 + 10", 6.0),
        ("-(2 + 3)", -5.0),
        ("10 % 4", 2.0),
        ("7 ÷ 2", 3.5),
        ("3 × −2", -6.0),
        ("0.1 + 0.2", 0.3),
    ] {
        let out = tool(&rt, "calculate", json!({ "expression": expression }), agent_ctx(0)).await.unwrap();
        assert_eq!(out["result"].as_f64(), Some(result), "{expression}");
    }
    for bad in ["", "1 +", "(1 + 2", "1 / 0", "2 $ 3", "1..2"] {
        assert!(tool(&rt, "calculate", json!({ "expression": bad }), agent_ctx(0)).await.is_err(), "{bad:?}");
    }
}

#[tokio::test]
async fn results_are_formatted_for_the_display() {
    let rt = calculator().await;
    for (expression, shown) in [
        ("1234567", "1,234,567"),
        ("7 / 2", "3.5"),
        ("1000000 * 1000000", "1e12"),
        ("987654321 * 999", "9.86666667e11"),
        ("0.00000012", "1.2e-7"),
    ] {
        let out = tool(&rt, "calculate", json!({ "expression": expression }), agent_ctx(0)).await.unwrap();
        assert_eq!(out["display"], shown, "{expression}");
    }
}

/// `%.9g` writes 1e-6..1e-4 and a value that rounds up to 1e9 in exponent form, which has no integer part:
/// `group()` used to raise on it, so `calculate { expression = "0.00005" }` and 1 / 20000 = crashed.
#[tokio::test]
async fn results_between_a_millionth_and_a_ten_thousandth_are_shown_in_full_not_a_crash() {
    let rt = calculator().await;
    for (expression, shown) in [
        ("0.00005", "0.00005"),
        ("1 / 20000", "0.00005"),
        ("0.0001", "0.0001"),
        ("0.000001", "0.000001"),
        ("-0.0000123456789", "-0.0000123456789"),
        ("0.0000999999999", "0.0000999999999"),
        ("999999999.6", "1e9"),
        ("-999999999.6", "-1e9"),
        ("999999999", "999,999,999"),
        ("1234567.891", "1,234,567.89"),
        ("0.1 + 0.2", "0.3"),
        ("0 * -5", "0"),
    ] {
        let out = tool(&rt, "calculate", json!({ "expression": expression }), agent_ctx(0))
            .await
            .unwrap_or_else(|e| panic!("{expression}: {e}"));
        assert_eq!(out["display"], shown, "{expression}");
    }
}

#[tokio::test]
async fn calculate_writes_history_so_it_is_not_read_only() {
    let rt = calculator().await;
    tool(&rt, "calculate", json!({ "expression": "2 + 2" }), agent_ctx(0)).await.unwrap();
    let history = tool(&rt, "history", json!({ "limit": 5 }), agent_ctx(0)).await.unwrap();
    assert_eq!(history["entries"][0], json!({ "expression": "2 + 2", "result": 4 }));
    let loaded = rt.windows(&id("calculator")).await.unwrap();
    assert_eq!(loaded, ["main"]);
}

#[tokio::test]
async fn only_the_tools_that_change_nothing_are_marked_read_only() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/calculator");
    let source = std::fs::read_to_string(dir.join("app.luau")).unwrap();
    // `calculate` and `show_on_calculator` write app.kv / app.state; `history` only reads
    let marked: Vec<&str> = source
        .split("app.tool {")
        .skip(1)
        .filter(|block| block.contains("readOnly = true"))
        .filter_map(|block| block.split('"').nth(1))
        .collect();
    assert_eq!(marked, ["history"]);
}

// ---- the keypad, driven through the real app's taps ------------------------------------------------------

/// A Calculator window: every key is a real tap on the node the app rendered, and the display, caption and
/// pending-operator state are read back from the next render.
struct Keypad {
    rt: LuauRuntime,
    out: RenderOutput,
}

fn find<'a>(node: &'a Node, key: &str) -> Option<&'a Node> {
    if node.key == key {
        return Some(node);
    }
    node.containers().flat_map(|(_, children)| children).find_map(|child| find(child, key))
}

/// The path of the node with this key (depth first).
fn path_of(node: &Node, key: &str) -> Option<String> {
    fn walk(node: &Node, path: &str, key: &str) -> Option<String> {
        if node.key == key {
            return Some(path.to_owned());
        }
        for (slot, children) in node.containers() {
            for child in children {
                let p = agentos_domain::ui::node::join_path(path, slot, &child.key);
                if let Some(found) = walk(child, &p, key) {
                    return Some(found);
                }
            }
        }
        None
    }
    walk(node, &node.key, key)
}

impl Keypad {
    async fn open() -> Self {
        let rt = calculator().await;
        let out = rt.open_view(&id("calculator"), "v1", "main", Map::new(), Map::new()).await.unwrap();
        Self { rt, out }
    }

    async fn fire(&mut self, key: &str, event: &str) {
        let path = path_of(&self.out.tree, key).unwrap_or_else(|| panic!("no node `{key}`"));
        let call = ViewEventCall { path, event: event.into(), row_key: None, row: None, payload: None };
        assert!(
            self.rt.dispatch_view_event(&id("calculator"), "v1", call, user_ctx(0)).await.unwrap(),
            "{key}"
        );
        self.refresh().await;
    }

    /// Tap keys by name: `7`, `dot`, `add`, `sub`, `mul`, `div`, `eq`, `clear`, `negate`, `percent`, and `back`
    /// (the Backspace shortcut).
    async fn press(&mut self, keys: &str) -> &mut Self {
        for name in keys.split_whitespace() {
            if name == "back" {
                self.fire("backspace", "trigger").await;
            } else {
                self.fire(&format!("key-{name}"), "tap").await;
            }
        }
        self
    }

    async fn refresh(&mut self) {
        self.out = self.rt.render_view(&id("calculator"), "v1").await.unwrap();
    }

    fn text(&self, key: &str) -> String {
        find(&self.out.tree, key)
            .and_then(|n| n.props.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("no text on `{key}`"))
            .to_owned()
    }

    fn display(&self) -> String {
        self.text("display")
    }

    fn caption(&self) -> String {
        self.text("caption")
    }

    /// The title the clear key shows: "C" while something is being typed, else "AC".
    fn clear_title(&self) -> String {
        let node = find(&self.out.tree, "key-clear").unwrap();
        node.props.get("title").and_then(Value::as_str).unwrap().to_owned()
    }

    /// The operator keys shown as pressed (`aria-pressed`).
    fn pending(&self) -> Vec<&'static str> {
        ["add", "sub", "mul", "div"]
            .into_iter()
            .filter(|op| {
                find(&self.out.tree, &format!("key-{op}")).and_then(|n| n.props.get("selected"))
                    == Some(&json!(true))
            })
            .collect()
    }
}

/// Press `keys` on a fresh Calculator and return what it shows.
async fn shows(keys: &str) -> String {
    Keypad::open().await.press(keys).await.display()
}

#[tokio::test]
async fn typing_builds_a_grouped_entry_with_one_dot_and_at_most_nine_digits() {
    for (keys, shown) in [
        ("", "0"),
        ("0 0 0", "0"),
        ("7", "7"),
        ("0 7", "7"),
        ("1 2 3 4 5 6 7", "1,234,567"),
        ("1 2 3 4 5 6 7 8 9 1 2", "123,456,789"),
        ("dot", "0."),
        ("dot 5", "0.5"),
        ("1 dot 5 dot 2", "1.52"),
        ("0 dot 0 0", "0.00"),
        ("1 2 3 4 dot 5 0", "1,234.50"),
    ] {
        assert_eq!(shows(keys).await, shown, "{keys}");
    }
}

#[tokio::test]
async fn simple_sums_and_the_caption() {
    for (keys, shown, caption) in [
        ("1 2 add 7 eq", "19", "12 + 7"),
        ("9 sub 4 eq", "5", "9 − 4"),
        ("6 mul 7 eq", "42", "6 × 7"),
        ("8 div 4 eq", "2", "8 ÷ 4"),
        ("1 div 3 eq", "0.333333333", "1 ÷ 3"),
        ("2 5 0 mul 0 dot 1 8 eq", "45", "250 × 0.18"),
        ("0 dot 1 add 0 dot 2 eq", "0.3", "0.1 + 0.2"),
        ("1 0 0 0 mul 1 0 0 0 eq", "1,000,000", "1,000 × 1,000"),
    ] {
        let mut pad = Keypad::open().await;
        pad.press(keys).await;
        assert_eq!((pad.display(), pad.caption()), (shown.to_owned(), caption.to_owned()), "{keys}");
    }
}

#[tokio::test]
async fn chained_operations_run_left_to_right_and_show_each_intermediate_result() {
    let mut pad = Keypad::open().await;
    pad.press("9 mul").await;
    assert_eq!((pad.display(), pad.pending()), ("9".into(), vec!["mul"]), "the pending operator is selected");
    pad.press("3").await;
    assert_eq!((pad.display(), pad.pending()), ("3".into(), vec![]), "typing the next number clears it");
    pad.press("sub").await;
    assert_eq!((pad.display(), pad.caption()), ("27".into(), "9 × 3".into()));
    pad.press("7 eq").await;
    assert_eq!((pad.display(), pad.caption()), ("20".into(), "27 − 7".into()));

    // no precedence: 2 + 3 × 4 is (2 + 3) × 4
    assert_eq!(shows("2 add 3 mul 4 eq").await, "20");
    assert_eq!(shows("1 add 2 add 3 add 4 eq").await, "10");
    // changing your mind about the operator before the next number does not calculate anything
    let mut pad = Keypad::open().await;
    pad.press("5 add sub").await;
    assert_eq!((pad.display(), pad.pending()), ("5".into(), vec!["sub"]));
    pad.press("2 eq").await;
    assert_eq!(pad.display(), "3");
    // a result carries on as the first operand; `=` with nothing pending does nothing
    assert_eq!(shows("2 add 3 eq mul 4 eq").await, "20");
    assert_eq!(shows("2 add 3 eq eq").await, "5");
    // `5 +` then `=` uses the displayed number twice, as the iPadOS calculator does
    assert_eq!(shows("5 add eq").await, "10");
}

#[tokio::test]
async fn a_result_is_replaced_by_the_next_digit_not_appended_to() {
    assert_eq!(shows("2 add 2 eq 7").await, "7");
    assert_eq!(shows("2 add 2 eq dot").await, "0.");
    assert_eq!(shows("2 add 2 eq 7 add 1 eq").await, "8");
}

#[tokio::test]
async fn plus_minus_flips_the_sign_of_what_is_shown() {
    for (keys, shown) in [
        ("negate", "0"),
        ("5 negate", "-5"),
        ("5 negate negate", "5"),
        ("1 2 3 4 negate", "-1,234"),
        ("negate 5", "5"),
        ("5 negate dot 5", "-5.5"),
        ("2 add 3 eq negate", "-5"),
        ("2 add 3 eq negate negate", "5"),
        ("5 sub 2 negate eq", "7"),
        ("5 negate mul 3 eq", "-15"),
        ("5 mul 3 negate eq", "-15"),
        ("3 sub 3 eq negate", "0"),
    ] {
        assert_eq!(shows(keys).await, shown, "{keys}");
    }
}

#[tokio::test]
async fn percent_divides_by_a_hundred_and_leaves_a_fresh_entry() {
    for (keys, shown) in [
        ("5 0 percent", "0.5"),
        ("7 percent", "0.07"),
        ("2 0 0 percent percent", "0.02"),
        ("5 0 percent mul 2 0 0 eq", "100"),
        ("5 0 percent add 1 eq", "1.5"),
    ] {
        assert_eq!(shows(keys).await, shown, "{keys}");
    }
    // the entry is fresh: the next digit starts a number instead of extending the result (it was "1e-065")
    assert_eq!(shows("0 dot 0 0 0 1 percent 5").await, "5");
    assert_eq!(shows("5 0 percent 5").await, "5");
    assert_eq!(shows("5 0 percent dot 5").await, "0.5");
    // and the result still works as an operand
    assert_eq!(shows("0 dot 0 0 0 1 percent mul 5 eq").await, "0.000005");
}

#[tokio::test]
async fn results_in_exponent_form_are_ordinary_operands() {
    for (keys, shown) in [
        // 1e-7 typed in full, then more arithmetic (was Error)
        ("0 dot 0 0 0 0 0 0 1 mul 2 eq", "2e-7"),
        ("0 dot 0 0 0 0 0 0 1 add 1 eq", "1.0000001"),
        ("0 dot 0 0 0 0 0 0 1 mul 2 eq mul 2 eq", "4e-7"),
        // 1 / 300000000 = 3.33e-9, then × 2 (was Error)
        ("1 div 3 0 0 0 0 0 0 0 0 eq", "3.33333333e-9"),
        ("1 div 3 0 0 0 0 0 0 0 0 eq mul 2 eq", "6.66666667e-9"),
        ("1 div 3 0 0 0 0 0 0 0 0 eq mul 3 eq", "1e-8"),
        // a result past nine digits is shown in exponent form and stays a number
        ("9 9 9 9 9 9 9 9 9 mul 9 9 9 9 9 9 9 9 9 eq", "9.99999998e17"),
        ("1 0 0 0 0 0 0 0 0 mul 1 0 0 0 0 0 0 0 0 eq add 1 eq", "1e16"),
        ("1 0 0 0 0 0 0 0 0 mul 1 0 0 0 0 0 0 0 0 eq div 2 eq", "5e15"),
        // ± and % on an exponent-sized result
        ("0 dot 0 0 0 0 0 0 1 mul 2 eq negate", "-2e-7"),
        ("0 dot 0 0 0 0 0 0 1 mul 2 eq negate add 1 eq", "0.9999998"),
        ("0 dot 0 0 0 0 0 0 1 percent", "1e-9"),
        ("0 dot 0 0 0 0 0 0 1 percent mul 1 0 0 0 0 0 0 0 0 eq", "0.1"),
    ] {
        assert_eq!(shows(keys).await, shown, "{keys}");
    }
}

#[tokio::test]
async fn a_result_is_shown_the_way_calculate_shows_it() {
    // 12 significant digits used to reach the display; the tool shows (and now the keypad shows) 9
    assert_eq!(shows("1 div 3 eq").await, "0.333333333");
    assert_eq!(shows("2 div 3 eq").await, "0.666666667");
    assert_eq!(shows("1 0 0 div 3 eq").await, "33.3333333");
    assert_eq!(shows("9 9 9 9 9 9 9 9 9 div 3 eq").await, "333,333,333");
    let rt = calculator().await;
    for (expression, keys) in
        [("1 / 3", "1 div 3 eq"), ("2 / 3", "2 div 3 eq"), ("100 / 3", "1 0 0 div 3 eq")]
    {
        let out = tool(&rt, "calculate", json!({ "expression": expression }), agent_ctx(0)).await.unwrap();
        assert_eq!(shows(keys).await, out["display"].as_str().unwrap(), "{expression}");
    }
}

#[tokio::test]
async fn division_by_zero_is_an_error_until_cleared() {
    for keys in [
        "8 div 0 eq",
        "8 div 0 add",
        "8 div 0 mul 2 eq",
        "0 div 0 eq",
        "5 add 3 div 0 eq",
        "5 add 3 div 0 add",
    ] {
        let mut pad = Keypad::open().await;
        pad.press(keys).await;
        assert_eq!(pad.display(), "Error", "{keys}");
        // keys other than clear are ignored while the error shows
        pad.press("5 add 5 eq negate percent back").await;
        assert_eq!(pad.display(), "Error", "{keys}: ignored");
        assert_eq!(pad.clear_title(), "AC", "{keys}");
        pad.press("clear").await;
        assert_eq!((pad.display(), pad.pending()), ("0".into(), vec![]), "{keys}: cleared");
        pad.press("4 add 4 eq").await;
        assert_eq!(pad.display(), "8", "{keys}: carry on");
    }
    // dividing zero is fine
    assert_eq!(shows("0 div 5 eq").await, "0");
}

#[tokio::test]
async fn a_result_too_large_for_any_display_is_an_error_not_inf() {
    let big = "9 9 9 9 9 9 9 9 9";
    let mut pad = Keypad::open().await;
    // each `× 999999999` adds nine digits; past 1e308 the product is not a number any more
    pad.press(&format!("{big} mul {big} eq")).await;
    for _ in 0..40 {
        pad.press(&format!("mul {big} eq")).await;
    }
    assert_eq!(pad.display(), "Error");
    pad.press("clear").await;
    assert_eq!(pad.display(), "0");
}

#[tokio::test]
async fn backspace_removes_the_last_typed_character_only() {
    for (keys, shown) in [
        ("back", "0"),
        ("1 2 3 back", "12"),
        ("1 2 3 back back back", "0"),
        ("1 2 3 back back back back", "0"),
        ("1 dot 5 back", "1."),
        ("1 dot 5 back back", "1"),
        ("1 2 3 4 back", "123"),
        // a negative single digit goes to 0, never to a bare "-"
        ("5 negate back", "0"),
        ("5 negate back add 3 eq", "3"),
        ("1 2 negate back", "-1"),
        ("dot 5 negate back back", "0"),
        // a result is not text: Backspace leaves it alone
        ("1 2 add 3 eq back", "15"),
        // after an operator it leaves the shown operand alone, then typing continues
        ("1 2 add back 3 eq", "15"),
        ("1 2 add 3 4 back eq", "15"),
    ] {
        assert_eq!(shows(keys).await, shown, "{keys}");
    }
}

#[tokio::test]
async fn clear_clears_the_entry_first_and_everything_the_second_time() {
    let mut pad = Keypad::open().await;
    assert_eq!(pad.clear_title(), "AC", "nothing typed");
    pad.press("5 add 3").await;
    assert_eq!((pad.clear_title(), pad.display()), ("C".into(), "3".into()), "something is being typed");
    pad.press("clear").await;
    assert_eq!((pad.clear_title(), pad.display(), pad.pending()), ("AC".into(), "0".into(), vec!["add"]));
    // the sum is still pending: 5 + 4
    pad.press("4 eq").await;
    assert_eq!((pad.display(), pad.caption()), ("9".into(), "5 + 4".into()));
    assert_eq!(pad.clear_title(), "AC", "a result is not typed text");
    // AC drops the pending operator and the caption too
    pad.press("clear").await;
    assert_eq!((pad.display(), pad.caption()), ("0".into(), String::new()));
    pad.press("5 add 3 clear clear").await;
    assert_eq!((pad.display(), pad.pending(), pad.caption()), ("0".into(), vec![], String::new()));
    pad.press("2 eq").await;
    assert_eq!(pad.display(), "2", "no pending operator survives AC");
    assert_eq!(shows("1 2 3 clear clear").await, "0");
}

#[tokio::test]
async fn the_agent_tool_and_the_keypad_share_one_calculator() {
    let mut pad = Keypad::open().await;
    tool(&pad.rt, "show_on_calculator", json!({ "expression": "1 / 300000000" }), agent_ctx(0))
        .await
        .unwrap();
    pad.refresh().await;
    assert_eq!((pad.display(), pad.caption()), ("3.33333333e-9".into(), "1 / 300000000".into()));
    assert_eq!(pad.clear_title(), "AC");
    // carry on from the agent's result, which is in exponent form
    pad.press("mul 3 eq").await;
    assert_eq!(pad.display(), "1e-8");
    tool(&pad.rt, "show_on_calculator", json!({ "expression": "18 / 100 * 2450" }), agent_ctx(0))
        .await
        .unwrap();
    pad.refresh().await;
    assert_eq!(pad.display(), "441");
    pad.press("7").await;
    assert_eq!(pad.display(), "7");
}
