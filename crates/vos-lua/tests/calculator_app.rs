#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The shipped Calculator (`apps/calculator`, not a fixture) under the real runtime: its arithmetic, its
//! number formatting through the tools, and what its tools declare to agents.

mod common;

use common::{agent_ctx, built_app, db, id, user_ctx};
use serde_json::{json, Map, Value};
use std::{path::Path, sync::Arc};
use vos_application::ports::{
    AppData, AppPackage, AppRuntime, CallContext, Notifier, PortResult, RenderOutput, ViewEffect,
    ViewEventCall,
};
use vos_domain::ui::{protocol::Command, Node};
use vos_lua::LuaRuntime;

struct Quiet;
#[async_trait::async_trait]
impl Notifier for Quiet {
    async fn notify(&self, _: vos_domain::Notification) -> PortResult<()> {
        Ok(())
    }
}

async fn calculator() -> LuaRuntime {
    calculator_from(built_app("calculator")).await
}

async fn calculator_from(package: AppPackage) -> LuaRuntime {
    let runtime =
        LuaRuntime::new(Arc::new(Quiet), Arc::new(vos_application::testing::RecordingShell::default()));
    let data: Arc<dyn AppData> = db();
    runtime.load(&id("calculator"), package, data).await.unwrap();
    runtime
}

async fn tool(rt: &LuaRuntime, name: &str, args: Value, ctx: CallContext) -> PortResult<Value> {
    rt.call_tool(&id("calculator"), name, args, ctx, false).await
}

#[tokio::test]
async fn calculate_follows_precedence_and_rejects_what_is_not_arithmetic() {
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
    rt: LuaRuntime,
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
                let p = vos_domain::ui::node::join_path(path, slot, &child.key);
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
        Self::open_on(calculator().await).await
    }

    async fn open_on(rt: LuaRuntime) -> Self {
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

    /// Multiply 999999999 by itself, again and again: each `× 999999999` adds nine digits, and past 1e308 the
    /// product is not a number any more. Stops at the error (a digit would start a new sum).
    async fn overflow(&mut self) {
        let big = "9 9 9 9 9 9 9 9 9";
        self.press(&format!("{big} mul {big} eq")).await;
        for _ in 0..40 {
            if self.display() == "Error" {
                break;
            }
            self.press(&format!("mul {big} eq")).await;
        }
    }

    /// What the window asked the client to announce to a screen reader since the last call, in order.
    async fn announcements(&self) -> Vec<String> {
        self.rt
            .take_view_effects(&id("calculator"))
            .await
            .unwrap()
            .into_iter()
            .filter_map(|(_, effect)| match effect {
                ViewEffect::Command(Command::Announce { text }) => Some(text),
                _ => None,
            })
            .collect()
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
async fn operators_of_equal_precedence_chain_left_to_right_and_show_each_intermediate_result() {
    let mut pad = Keypad::open().await;
    pad.press("9 mul").await;
    assert_eq!((pad.display(), pad.pending()), ("9".into(), vec!["mul"]), "the pending operator is selected");
    pad.press("3").await;
    assert_eq!((pad.display(), pad.pending()), ("3".into(), vec![]), "typing the next number clears it");
    pad.press("sub").await;
    assert_eq!(
        (pad.display(), pad.caption()),
        ("27".into(), "9 × 3 −".into()),
        "9 × 3 is complete once a lower operator comes, so it is shown; the caption is the sum so far"
    );
    pad.press("7").await;
    assert_eq!(
        (pad.display(), pad.caption()),
        ("7".into(), "9 × 3 −".into()),
        "and the caption keeps saying it while typing"
    );
    pad.press("eq").await;
    assert_eq!((pad.display(), pad.caption()), ("20".into(), "9 × 3 − 7".into()));

    assert_eq!(shows("1 add 2 add 3 add 4 eq").await, "10");
    assert_eq!(shows("8 sub 2 sub 1 eq").await, "5", "subtraction is left to right");
    assert_eq!(shows("8 div 4 div 2 eq").await, "1", "so is division");
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

/// The iPadOS Calculator's way: a higher operator waits (`2 + 3 ×` still shows 3), a lower or equal one completes
/// what is waiting (`2 × 3 +` shows 6), and `=` evaluates the whole sum.
#[tokio::test]
async fn multiplication_and_division_wait_for_their_second_number_before_addition_and_subtraction() {
    let mut pad = Keypad::open().await;
    pad.press("2 add 3 mul").await;
    assert_eq!(
        (pad.display(), pad.caption(), pad.pending()),
        ("3".into(), "2 + 3 ×".into(), vec!["mul"]),
        "nothing is reduced yet"
    );
    pad.press("4").await;
    assert_eq!((pad.display(), pad.caption(), pad.pending()), ("4".into(), "2 + 3 ×".into(), vec![]));
    pad.press("eq").await;
    assert_eq!((pad.display(), pad.caption(), pad.pending()), ("14".into(), "2 + 3 × 4".into(), vec![]));

    for (keys, display, caption) in [
        ("2 mul 3 add", "6", "2 × 3 +"),
        ("2 add 3 mul 4 add", "14", "2 + 3 × 4 +"),
        ("2 add 3 mul 4 sub", "14", "2 + 3 × 4 −"),
        ("2 add 3 mul 4 mul", "12", "2 + 3 × 4 ×"),
        ("2 add 8 div 4 div", "2", "2 + 8 ÷ 4 ÷"),
        ("2 sub 3 mul 4 div", "12", "2 − 3 × 4 ÷"),
        ("2 add 3 sub", "5", "2 + 3 −"),
    ] {
        let mut pad = Keypad::open().await;
        pad.press(keys).await;
        assert_eq!((pad.display(), pad.caption()), (display.to_owned(), caption.to_owned()), "{keys}");
    }
    for (keys, shown) in [
        ("2 add 3 mul 4 mul 5 eq", "62"),
        ("2 mul 3 add 4 eq", "10"),
        ("2 sub 3 mul 4 sub 5 eq", "-15"),
        // the displayed number is the operand of `=`, as with `5 + =`
        ("2 add 3 mul eq", "11"),
        ("2 mul 3 add eq", "12"),
        ("2 add 3 mul 4 add eq", "28"),
    ] {
        assert_eq!(shows(keys).await, shown, "{keys}");
    }
}

/// Changing your mind about the operator before the next number re-reads the sum with the new one: nothing was
/// worked out for the replaced operator.
#[tokio::test]
async fn changing_the_operator_before_the_next_number_re_reads_the_sum_with_the_new_one() {
    for (keys, display, caption, pending) in [
        ("2 add 3 mul add", "5", "2 + 3 +", "add"),
        ("2 mul 3 add mul", "6", "2 × 3 ×", "mul"),
        ("2 add 3 mul sub", "5", "2 + 3 −", "sub"),
        ("2 add 3 mul div", "3", "2 + 3 ÷", "div"),
        ("2 add mul", "2", "2 ×", "mul"),
    ] {
        let mut pad = Keypad::open().await;
        pad.press(keys).await;
        assert_eq!(
            (pad.display(), pad.caption(), pad.pending()),
            (display.to_owned(), caption.to_owned(), vec![pending]),
            "{keys}"
        );
    }
    for (keys, shown) in [
        ("2 add 3 mul add 4 eq", "9"),
        ("2 add 3 add mul 4 eq", "14"),
        ("2 add 3 mul sub 4 eq", "1"),
        ("2 add add add 3 eq", "5"),
    ] {
        assert_eq!(shows(keys).await, shown, "{keys}");
    }
}

/// `=` finishes a sum and nothing repeats it: a second `=` does nothing (the iPadOS Calculator repeats the last
/// operation; this one never did, and the change is only about how a sum is evaluated).
#[tokio::test]
async fn equals_finishes_the_sum_and_a_second_equals_does_nothing() {
    let mut pad = Keypad::open().await;
    pad.press("2 add 3 mul 4 eq").await;
    assert_eq!((pad.display(), pad.caption()), ("14".into(), "2 + 3 × 4".into()));
    pad.press("eq eq").await;
    assert_eq!((pad.display(), pad.caption(), pad.pending()), ("14".into(), "2 + 3 × 4".into(), vec![]));
    assert_eq!(shows("eq").await, "0", "nothing to finish");
    assert_eq!(shows("7 eq").await, "7", "a typed number alone");
}

/// A result is the first number of the next sum, which has its own precedence; so is a result an agent put there.
#[tokio::test]
async fn a_chain_can_start_from_the_previous_result() {
    for (keys, shown, caption) in [
        ("2 add 3 mul 4 eq add 1 mul 2 eq", "16", "14 + 1 × 2"),
        ("2 add 3 mul 4 eq mul 2 add 1 eq", "29", "14 × 2 + 1"),
        ("2 add 3 eq mul 4 add 1 eq", "21", "5 × 4 + 1"),
        ("2 add 3 mul 4 eq sub 1 sub 1 eq", "12", "14 − 1 − 1"),
    ] {
        let mut pad = Keypad::open().await;
        pad.press(keys).await;
        assert_eq!((pad.display(), pad.caption()), (shown.to_owned(), caption.to_owned()), "{keys}");
    }
    let mut pad = Keypad::open().await;
    tool(&pad.rt, "show_on_calculator", json!({ "expression": "2 + 3 * 4" }), agent_ctx(0)).await.unwrap();
    pad.refresh().await;
    assert_eq!(pad.display(), "14");
    pad.press("add 1 mul").await;
    assert_eq!((pad.display(), pad.caption()), ("1".into(), "14 + 1 ×".into()));
    pad.press("2 eq").await;
    assert_eq!((pad.display(), pad.caption()), ("16".into(), "14 + 1 × 2".into()));
}

/// Chains and mixed operators typed on the keypad, and the same text given to `calculate`: they run one evaluator
/// (`calc.luau`), so the display must be the same however the sum arrives.
#[tokio::test]
async fn the_keypad_and_the_calculate_tool_evaluate_alike() {
    for (keys, expression, shown) in [
        ("2 add 3 mul 4 eq", "2 + 3 × 4", "14"),
        ("2 mul 3 add 4 eq", "2 × 3 + 4", "10"),
        ("8 sub 2 sub 1 eq", "8 − 2 − 1", "5"),
        ("8 div 4 div 2 eq", "8 ÷ 4 ÷ 2", "1"),
        ("2 add 3 mul 4 sub 5 div 5 eq", "2 + 3 × 4 − 5 ÷ 5", "13"),
        ("1 0 sub 2 mul 3 eq", "10 − 2 × 3", "4"),
        ("1 0 sub 2 mul 3 sub 1 eq", "10 − 2 × 3 − 1", "3"),
        ("1 2 div 4 add 2 mul 3 eq", "12 ÷ 4 + 2 × 3", "9"),
        ("1 div 3 add 1 div 3 eq", "1 / 3 + 1 / 3", "0.666666667"),
        // decimals
        ("0 dot 1 add 0 dot 2 mul 3 eq", "0.1 + 0.2 × 3", "0.7"),
        ("1 dot 5 mul 2 add 0 dot 2 5 eq", "1.5 × 2 + 0.25", "3.25"),
        ("1 0 0 0 mul 1 0 0 0 add 1 eq", "1000 × 1000 + 1", "1,000,001"),
        ("0 dot 0 0 0 0 0 0 1 add 1 mul 2 eq", "0.0000001 + 1 × 2", "2.0000001"),
        // ± is the minus sign of the number typed, `−` the operator
        ("2 sub 5 negate eq", "2 − −5", "7"),
        ("5 negate mul 3 add 1 eq", "-5 × 3 + 1", "-14"),
        ("3 sub 2 mul 5 negate eq", "3 − 2 × -5", "13"),
        ("2 add 3 mul 4 negate eq", "2 + 3 × -4", "-10"),
        ("2 add 3 mul negate eq", "2 + 3 × -3", "-7"),
        // % on the keypad is "divide by 100"
        ("5 0 percent mul 4 add 1 eq", "50 / 100 × 4 + 1", "3"),
        ("2 add 5 0 percent mul 4 eq", "2 + 50 / 100 × 4", "4"),
        ("2 0 0 mul 5 percent add 1 eq", "200 × (5 / 100) + 1", "11"),
        // a long chain
        ("1 add 2 mul 3 add 4 mul 5 add 6 mul 7 add 8 mul 9 eq", "1 + 2 × 3 + 4 × 5 + 6 × 7 + 8 × 9", "141"),
        (
            "1 add 2 add 3 add 4 add 5 add 6 add 7 add 8 add 9 add 1 0 eq",
            "1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 10",
            "55",
        ),
        ("1 0 0 sub 1 sub 2 sub 3 div 3 mul 2 sub 1 eq", "100 − 1 − 2 − 3 ÷ 3 × 2 − 1", "94"),
    ] {
        let mut pad = Keypad::open().await;
        pad.press(keys).await;
        let out =
            tool(&pad.rt, "calculate", json!({ "expression": expression }), agent_ctx(0)).await.unwrap();
        assert_eq!(pad.display(), shown, "keypad: {keys}");
        assert_eq!(out["display"], shown, "calculate: {expression}");
    }
}

/// The keys that type a whole number: `12` is `1 2`.
fn typed(number: u32) -> String {
    number.to_string().chars().map(String::from).collect::<Vec<_>>().join(" ")
}

/// Not a handful of examples: every sum of four numbers with every combination of the three operators, so a
/// difference in precedence or associativity between the keypad and the tool cannot hide.
#[tokio::test]
async fn every_four_number_sum_gives_the_same_on_the_keypad_and_from_the_tool() {
    const OPS: [(&str, &str); 4] = [("add", "+"), ("sub", "-"), ("mul", "*"), ("div", "/")];
    let mut pad = Keypad::open().await;
    let mut compared = 0;
    for (a, b, c, d) in [(12, 3, 4, 2), (7, 5, 9, 6)] {
        for (op1, sym1) in OPS {
            for (op2, sym2) in OPS {
                for (op3, sym3) in OPS {
                    let (ka, kb, kc, kd) = (typed(a), typed(b), typed(c), typed(d));
                    let keys = format!("{ka} {op1} {kb} {op2} {kc} {op3} {kd} eq");
                    let expression = format!("{a} {sym1} {b} {sym2} {c} {sym3} {d}");
                    pad.press("clear clear").await;
                    pad.press(&keys).await;
                    let out = tool(&pad.rt, "calculate", json!({ "expression": expression }), agent_ctx(0))
                        .await
                        .unwrap();
                    assert_eq!(pad.display(), out["display"].as_str().unwrap(), "{expression} ({keys})");
                    compared += 1;
                }
            }
        }
    }
    assert_eq!(compared, 128);
}

/// A failure inside a mixed sum is the same sentence from the keypad and from the tool.
#[tokio::test]
async fn a_failing_mixed_sum_gives_the_same_reason_on_the_keypad_and_from_the_tool() {
    for (keys, expression) in [
        ("8 div 0 eq", "8 / 0"),
        ("5 add 3 div 0 eq", "5 + 3 / 0"),
        ("1 add 2 mul 3 div 0 eq", "1 + 2 * 3 / 0"),
        ("4 div 2 sub 2 div 0 eq", "4 / 2 - 2 / 0"),
    ] {
        let mut pad = Keypad::open().await;
        pad.press(keys).await;
        let attempt = tool(&pad.rt, "calculate", json!({ "expression": expression }), agent_ctx(0)).await;
        assert_eq!(
            (pad.display(), pad.caption()),
            ("Error".into(), "Can't divide by zero".into()),
            "keypad: {keys}"
        );
        assert!(attempt.unwrap_err().to_string().contains("Can't divide by zero"), "calculate: {expression}");
    }

    // 1 + 999999999 × 999999999 × ...: the keypad stops at the press that cannot be shown, the tool at the same sum
    let nine = "9 9 9 9 9 9 9 9 9";
    let mut pad = Keypad::open().await;
    pad.press(&format!("1 add {nine}")).await;
    let mut factors = 1;
    while pad.display() != "Error" && factors < 45 {
        pad.press("mul").await; // an error is raised here; a digit after it would start a new sum
        if pad.display() != "Error" {
            pad.press(nine).await;
            factors += 1;
        }
    }
    assert_eq!((pad.display(), pad.caption()), ("Error".into(), "Result too large".into()));
    let expression = format!("1 + {}", vec!["999999999"; factors].join(" * "));
    let err =
        tool(&pad.rt, "calculate", json!({ "expression": expression }), agent_ctx(0)).await.unwrap_err();
    assert!(err.to_string().contains("Result too large"), "{err}");
}

/// A division by zero in the part an operator completes is raised when that operator is pressed (as `8 ÷ 0 +` is),
/// not later; one that is still waiting (`1 + 8 ÷ 0` before the next operator) is raised by `=` or by the next
/// operator, and the error clears like any other.
#[tokio::test]
async fn a_mixed_sum_fails_when_the_part_that_cannot_be_worked_out_is_complete() {
    for keys in [
        "1 add 8 div 0 add",
        "1 add 8 div 0 sub",
        "1 add 8 div 0 mul",
        "1 add 8 div 0 div",
        "1 add 8 div 0 eq",
        "2 mul 3 add 8 div 0 eq",
    ] {
        let mut pad = Keypad::open().await;
        pad.press(keys).await;
        assert_eq!((pad.display(), pad.caption()), ("Error".into(), "Can't divide by zero".into()), "{keys}");
        assert_eq!(pad.pending(), Vec::<&str>::new(), "{keys}: nothing of the failed sum is left");
        pad.press("clear 2 add 3 mul 4 eq").await;
        assert_eq!(pad.display(), "14", "{keys}: a new sum after the error");
    }
    let mut pad = Keypad::open().await;
    pad.press("1 add 8 div 0").await;
    assert_eq!(pad.display(), "0", "typing the divisor does not fail yet");
    // a zero that is not a divisor is fine, and a zero divided is too
    assert_eq!(shows("1 add 0 mul 8 add 0 div 5 eq").await, "1");
}

/// The caption is one line: a long chain keeps its last terms behind an ellipsis, and the key and the display
/// still tell the rest.
#[tokio::test]
async fn a_long_chain_keeps_the_caption_to_its_last_terms() {
    let mut pad = Keypad::open().await;
    pad.press("1 add 2 add 3 add 4 add 5 add 6 add 7 add").await;
    assert_eq!((pad.display(), pad.caption()), ("28".into(), "… 2 + 3 + 4 + 5 + 6 + 7 +".into()));
    pad.press("8 eq").await;
    assert_eq!((pad.display(), pad.caption()), ("36".into(), "… 3 + 4 + 5 + 6 + 7 + 8".into()));
    // a short sum is written in full, even with long numbers
    pad.press("clear 1 2 3 4 5 6 7 8 9 mul 9 9 9 add").await;
    assert_eq!(pad.caption(), "123,456,789 × 999 +", "two long numbers still fit");
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
        // in a pending sum the displayed number becomes the operand
        ("5 add negate eq", "0"),
        ("2 add 3 mul negate eq", "-7"),
        ("2 add 3 mul 4 negate eq", "-10"),
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
        ("2 add 5 0 percent eq", "2.5"),
        ("2 add 3 mul 5 0 percent eq", "3.5"),
        ("2 add 3 mul percent eq", "2.09"),
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
    for keys in ["8 div 0 eq", "8 div 0 add", "0 div 0 eq", "5 add 3 div 0 eq", "5 add 3 div 0 add"] {
        let mut pad = Keypad::open().await;
        pad.press(keys).await;
        assert_eq!(pad.display(), "Error", "{keys}");
        // keys with nothing to work on are ignored while the error shows
        pad.press("add sub mul div eq negate percent back").await;
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
    let mut pad = Keypad::open().await;
    pad.overflow().await;
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

/// `C` in the middle of a mixed sum clears the number being typed and leaves the sum going; `AC` drops it.
#[tokio::test]
async fn clear_entry_in_a_mixed_sum_keeps_the_sum_and_all_clear_drops_it() {
    let mut pad = Keypad::open().await;
    pad.press("2 add 3 mul 4 clear").await;
    assert_eq!(
        (pad.clear_title(), pad.display(), pad.caption(), pad.pending()),
        ("AC".into(), "0".into(), "2 + 3 ×".into(), vec!["mul"]),
        "the 4 is gone, the sum is not"
    );
    pad.press("5 eq").await;
    assert_eq!((pad.display(), pad.caption()), ("17".into(), "2 + 3 × 5".into()));

    // an operator before the next number replaces the waiting one, as it does without a clear
    assert_eq!(shows("2 add 3 mul 4 clear add 5 eq").await, "10");
    // `=` takes the cleared 0 as the operand
    assert_eq!(shows("2 add 3 mul 4 clear eq").await, "2");

    // the second clear is AC: the whole sum goes, and nothing of it comes back
    let mut pad = Keypad::open().await;
    pad.press("2 add 3 mul 4 clear clear").await;
    assert_eq!((pad.display(), pad.caption(), pad.pending()), ("0".into(), String::new(), vec![]));
    pad.press("6 eq").await;
    assert_eq!(pad.display(), "6");
    // AC while an operator waits (the display shows what it completed, nothing is being typed) clears it all
    let mut pad = Keypad::open().await;
    pad.press("2 add 3 mul clear").await;
    assert_eq!((pad.display(), pad.caption(), pad.pending()), ("0".into(), String::new(), vec![]));
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

#[tokio::test]
async fn a_digit_after_an_error_starts_a_new_sum() {
    for (keys, shown) in [
        ("8 div 0 eq 5", "5"),
        ("8 div 0 eq dot", "0."),
        ("8 div 0 eq 5 add 4 eq", "9"),
        ("8 div 0 add 7 mul 2 eq", "14"),
    ] {
        let mut pad = Keypad::open().await;
        pad.press(keys).await;
        assert_eq!(pad.display(), shown, "{keys}");
    }
    let mut pad = Keypad::open().await;
    pad.press("8 div 0 eq 5").await;
    assert_eq!((pad.caption(), pad.pending()), (String::new(), vec![]), "nothing of the failed sum is left");
}

#[tokio::test]
async fn the_pending_operator_is_in_the_caption_and_on_its_key() {
    let mut pad = Keypad::open().await;
    pad.press("2 add").await;
    assert_eq!((pad.caption(), pad.pending()), ("2 +".into(), vec!["add"]));
    pad.press("mul").await;
    assert_eq!((pad.caption(), pad.pending()), ("2 ×".into(), vec!["mul"]), "changing your mind");
    pad.press("3 sub").await;
    assert_eq!((pad.display(), pad.caption(), pad.pending()), ("6".into(), "2 × 3 −".into(), vec!["sub"]));
    pad.press("eq").await;
    assert_eq!((pad.display(), pad.caption(), pad.pending()), ("0".into(), "2 × 3 − 6".into(), vec![]));
    // typing a number, or turning the display into one with ± or %, ends the wait: the key is no longer pressed
    for keys in ["2 add 3", "2 add negate", "2 add percent"] {
        let mut pad = Keypad::open().await;
        pad.press(keys).await;
        assert_eq!(pad.pending(), Vec::<&str>::new(), "{keys}");
    }
}

// ---- error states: what the person and the agent are told --------------------------------------------------

#[tokio::test]
async fn a_failed_calculation_says_why_under_the_error() {
    for keys in ["8 div 0 eq", "8 div 0 add", "0 div 0 eq"] {
        let mut pad = Keypad::open().await;
        pad.press(keys).await;
        assert_eq!((pad.display(), pad.caption()), ("Error".into(), "Can't divide by zero".into()), "{keys}");
    }
    let mut pad = Keypad::open().await;
    pad.overflow().await;
    assert_eq!((pad.display(), pad.caption()), ("Error".into(), "Result too large".into()));
    // AC clears the reason with the error
    pad.press("clear").await;
    assert_eq!((pad.display(), pad.caption()), ("0".into(), String::new()));
}

/// The reason is read out by the window's always-present announcer (`w:announce`), not by a live region that
/// appears together with its text, which a screen reader often misses.
#[tokio::test]
async fn the_error_reason_is_announced_by_the_window_and_an_ordinary_caption_is_not() {
    let live = |pad: &Keypad| find(&pad.out.tree, "caption").and_then(|n| n.props.get("live")).cloned();
    let mut pad = Keypad::open().await;
    pad.press("1 2 add 7 eq").await;
    assert_eq!(pad.announcements().await, Vec::<String>::new(), "a caption changes on every sum");
    pad.press("div 0 eq").await;
    assert_eq!(pad.announcements().await, ["Can't divide by zero"]);
    assert_eq!(live(&pad), None, "the caption is never a live region: it would appear with its own text");
    // keys that are ignored while the error shows say nothing more; AC clears the announcer, so the same
    // reason is read again the next time it happens
    pad.press("add eq").await;
    assert_eq!(pad.announcements().await, Vec::<String>::new());
    pad.press("clear").await;
    assert_eq!(pad.announcements().await, [""]);
    pad.press("5 div 0 eq").await;
    assert_eq!(pad.announcements().await, ["Can't divide by zero"]);
    // a digit is the other way out
    pad.press("7").await;
    assert_eq!(pad.announcements().await, [""]);
    pad.press("add 2 eq").await;
    assert_eq!(pad.announcements().await, Vec::<String>::new());
}

/// An agent putting a result on the Calculator takes the error away, so it empties the announcer too: the same error
/// is read out again when it comes back.
#[tokio::test]
async fn show_on_calculator_empties_the_announcer_so_a_repeated_error_is_read_again() {
    let mut pad = Keypad::open().await;
    pad.press("8 div 0 eq").await;
    assert_eq!(pad.announcements().await, ["Can't divide by zero"]);
    tool(&pad.rt, "show_on_calculator", json!({ "expression": "2 + 2" }), agent_ctx(0)).await.unwrap();
    assert_eq!(pad.announcements().await, [""], "the tool empties the announcer of the open window");
    pad.refresh().await;
    assert_eq!(pad.display(), "4");
    pad.press("8 div 0 eq").await;
    assert_eq!(pad.announcements().await, ["Can't divide by zero"]);
}

/// `app:windows` reaches every open window of the Calculator, not only one.
#[tokio::test]
async fn show_on_calculator_empties_the_announcer_of_every_open_window() {
    let pad = Keypad::open().await;
    pad.rt.open_view(&id("calculator"), "v2", "main", Map::new(), Map::new()).await.unwrap();
    tool(&pad.rt, "show_on_calculator", json!({ "expression": "2 + 2" }), agent_ctx(0)).await.unwrap();
    let cleared = pad.rt.take_view_effects(&id("calculator")).await.unwrap();
    let mut views: Vec<_> = cleared
        .into_iter()
        .filter(|(_, effect)| matches!(effect, ViewEffect::Command(Command::Announce { text }) if text.is_empty()))
        .map(|(view, _)| view)
        .collect();
    views.sort();
    assert_eq!(views, ["v1", "v2"]);
}

/// A bug in the app (not a failure `calc.luau` raises on purpose) must not show a source position to the person.
#[tokio::test]
async fn an_unexpected_error_shows_a_generic_sentence_not_a_source_position() {
    let mut package = built_app("calculator");
    let source = String::from_utf8(package.files["app.luau"].clone()).unwrap();
    assert!(source.contains("return a / b"), "the bundle changed: update the anchor");
    package
        .files
        .insert("app.luau".into(), source.replacen("return a / b", "return a / nil", 1).into_bytes());
    let mut pad = Keypad::open_on(calculator_from(package).await).await;
    pad.press("8 div 2 eq").await;
    assert_eq!((pad.display(), pad.caption()), ("Error".into(), "Something went wrong".into()));
    assert_eq!(pad.announcements().await, ["Something went wrong"]);
    pad.press("clear 4 add 4 eq").await;
    assert_eq!(pad.display(), "8", "the keypad is usable again");
}

#[tokio::test]
async fn the_equals_sign_on_the_keyboard_is_equals() {
    let mut pad = Keypad::open().await;
    pad.press("6 mul 7").await;
    pad.fire("equals", "trigger").await;
    assert_eq!((pad.display(), pad.caption()), ("42".into(), "6 × 7".into()));
    // a typed `=` finishes a mixed sum the same way, and the keys of the pad are the keys of the keyboard
    let mut pad = Keypad::open().await;
    pad.press("2 add 3 mul 4").await;
    pad.fire("equals", "trigger").await;
    assert_eq!((pad.display(), pad.caption()), ("14".into(), "2 + 3 × 4".into()));
    pad.press("back").await;
    assert_eq!(pad.display(), "14", "Backspace leaves a result alone");
}

/// An agent gets the reason as a plain sentence, not a source position.
#[tokio::test]
async fn calculate_rejects_what_cannot_be_shown_with_a_plain_reason() {
    let rt = calculator().await;
    for (expression, reason) in [
        ("1 / 0", "Can't divide by zero"),
        ("5 % 0", "Can't divide by zero"),
        ("10 ^ 400", "Result too large"),
        ("(-8) ^ 0.5", "Not a real number"),
        ("(1 + 2", "Mismatched parentheses"),
        ("1 +", "Malformed expression"),
        ("", "Empty expression"),
        ("2 $ 3", "Unexpected character \"$\""),
    ] {
        let err = tool(&rt, "calculate", json!({ "expression": expression }), agent_ctx(0))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains(reason), "{expression:?}: {err}");
        assert!(!err.contains("calc.lua"), "{expression:?} leaks a source position: {err}");
    }
    let history = tool(&rt, "history", json!({}), agent_ctx(0)).await.unwrap();
    assert_eq!(history["entries"], json!([]), "a failed calculation is not history");
}

#[tokio::test]
async fn a_failed_show_on_calculator_leaves_the_calculator_alone() {
    let mut pad = Keypad::open().await;
    pad.press("1 2 add 3 eq").await;
    let attempt = tool(&pad.rt, "show_on_calculator", json!({ "expression": "1 / 0" }), agent_ctx(0)).await;
    assert!(attempt.is_err());
    pad.refresh().await;
    assert_eq!((pad.display(), pad.caption()), ("15".into(), "12 + 3".into()));
}

#[tokio::test]
async fn history_is_newest_first_capped_and_limited_by_the_caller() {
    let rt = calculator().await;
    for n in 1..=55 {
        tool(&rt, "calculate", json!({ "expression": format!("{n} + 0") }), agent_ctx(0)).await.unwrap();
    }
    let ten = tool(&rt, "history", json!({}), agent_ctx(0)).await.unwrap();
    assert_eq!(ten["entries"].as_array().unwrap().len(), 10, "the default limit");
    assert_eq!(ten["entries"][0], json!({ "expression": "55 + 0", "result": 55 }));
    let all = tool(&rt, "history", json!({ "limit": 50 }), agent_ctx(0)).await.unwrap();
    assert_eq!(all["entries"].as_array().unwrap().len(), 50, "the app keeps the newest 50");
    assert_eq!(all["entries"][49]["result"], 6);
    assert!(tool(&rt, "history", json!({ "limit": 0 }), agent_ctx(0)).await.is_err(), "bounds are enforced");
    assert!(tool(&rt, "history", json!({ "limit": 51 }), agent_ctx(0)).await.is_err());
}
