#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration tests
//! The Calculator built as a Vite project (`apps/calculator-web`): its contract's one method, `press`, through the
//! generated router, under the real runtime. The backend keeps nothing: each call carries the keypad's state and answers
//! with the next one. The tables are the old Calculator's (`calculator_app.rs`), so the two keypads are held to the same
//! behaviour while both exist.

mod common;

use common::{built_package, db, id, user_ctx};
use serde_json::{json, Value};
use std::{
    path::Path,
    process::Command,
    sync::{Arc, Once},
};
use vos_application::{
    ports::{AppData, AppPackage, AppRuntime, Notifier, PortResult},
    PortError,
};
use vos_domain::Notification;
use vos_lua::LuaRuntime;

const APP: &str = "calculator-web";

struct Quiet;
#[async_trait::async_trait]
impl Notifier for Quiet {
    async fn notify(&self, _: Notification) -> PortResult<()> {
        Ok(())
    }
}

/// The app's generated files are not committed: generate them once, as the project's own scripts do.
fn generate() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps").join(APP);
        let run = Command::new("bun")
            .args(["run", "generate"])
            .current_dir(&project)
            .output()
            .expect("bun is needed to generate the app's router (https://bun.sh)");
        assert!(run.status.success(), "bun run generate failed:\n{}", String::from_utf8_lossy(&run.stderr));
    });
}

async fn calculator() -> LuaRuntime {
    generate();
    let entry = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps").join(APP).join(".app/entry.luau");
    let package = AppPackage { has_client: true, ..built_package(&entry) };
    let runtime =
        LuaRuntime::new(Arc::new(Quiet), Arc::new(vos_application::testing::RecordingShell::default()));
    let data: Arc<dyn AppData> = db();
    runtime.load(&id(APP), package, data).await.unwrap();
    runtime
}

async fn call(rt: &LuaRuntime, args: Value) -> PortResult<Value> {
    rt.call_handler(&id(APP), "press", args, user_ctx(0)).await
}

/// A keypad as the client drives it: it keeps the last screen and sends its state with the next key.
struct Keypad {
    rt: LuaRuntime,
    screen: Value,
}

impl Keypad {
    async fn open() -> Self {
        Self { rt: calculator().await, screen: Value::Null }
    }

    /// Press keys by name: `7`, `dot`, `add`, `sub`, `mul`, `div`, `eq`, `clear`, `negate`, `percent`, `back`.
    async fn press(&mut self, keys: &str) -> &mut Self {
        for name in keys.split_whitespace() {
            let key = match name {
                "dot" => ".",
                "add" => "+",
                "sub" => "-",
                "mul" => "*",
                "div" => "/",
                "eq" => "=",
                "back" => "backspace",
                other => other,
            };
            let mut args = json!({ "key": key });
            if let Some(state) = self.screen.get("state") {
                args["state"] = state.clone();
            }
            self.screen = call(&self.rt, args).await.unwrap_or_else(|e| panic!("{name}: {e:?}"));
        }
        self
    }

    fn text(&self, field: &str) -> String {
        self.screen.get(field).and_then(Value::as_str).unwrap_or_default().to_owned()
    }

    fn display(&self) -> String {
        if self.screen.is_null() {
            return "0".into();
        }
        self.text("display")
    }

    fn caption(&self) -> String {
        self.text("caption")
    }

    /// The clear key's label: "C" while something is being typed, else "AC".
    fn clear_title(&self) -> String {
        if self.screen.is_null() {
            return "AC".into();
        }
        self.text("clear")
    }

    /// The operator keys drawn pressed.
    fn pending(&self) -> Vec<&'static str> {
        match self.screen.get("pending").and_then(Value::as_str) {
            Some("+") => vec!["add"],
            Some("-") => vec!["sub"],
            Some("*") => vec!["mul"],
            Some("/") => vec!["div"],
            _ => vec![],
        }
    }

    /// What the client announces: the reason for an error, else nothing.
    fn announced(&self) -> String {
        self.text("error")
    }

    /// Multiply 999999999 by itself until the product is not a number any more. Stops at the error.
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
}

/// Press `keys` on a fresh Calculator and return what it shows.
async fn shows(keys: &str) -> String {
    Keypad::open().await.press(keys).await.display()
}

#[tokio::test]
async fn typing_builds_a_grouped_entry_with_one_dot_and_at_most_nine_digits() {
    for (keys, shown) in [
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
    assert_eq!((pad.display(), pad.pending()), ("9".into(), vec!["mul"]), "the pending operator is pressed");
    pad.press("3").await;
    assert_eq!((pad.display(), pad.pending()), ("3".into(), vec![]), "typing the next number clears it");
    pad.press("sub").await;
    assert_eq!((pad.display(), pad.caption()), ("27".into(), "9 × 3 −".into()));
    pad.press("7").await;
    assert_eq!(
        (pad.display(), pad.caption()),
        ("7".into(), "9 × 3 −".into()),
        "the caption stays while typing"
    );
    pad.press("eq").await;
    assert_eq!((pad.display(), pad.caption()), ("20".into(), "9 × 3 − 7".into()));

    assert_eq!(shows("1 add 2 add 3 add 4 eq").await, "10");
    assert_eq!(shows("8 sub 2 sub 1 eq").await, "5", "subtraction is left to right");
    assert_eq!(shows("8 div 4 div 2 eq").await, "1", "so is division");
    let mut pad = Keypad::open().await;
    pad.press("5 add sub").await;
    assert_eq!((pad.display(), pad.pending()), ("5".into(), vec!["sub"]));
    pad.press("2 eq").await;
    assert_eq!(pad.display(), "3");
    assert_eq!(shows("2 add 3 eq mul 4 eq").await, "20");
    assert_eq!(shows("2 add 3 eq eq").await, "5");
    assert_eq!(shows("5 add eq").await, "10");
}

#[tokio::test]
async fn multiplication_and_division_wait_for_their_second_number_before_addition_and_subtraction() {
    let mut pad = Keypad::open().await;
    pad.press("2 add 3 mul").await;
    assert_eq!((pad.display(), pad.caption(), pad.pending()), ("3".into(), "2 + 3 ×".into(), vec!["mul"]));
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
        ("2 add 3 mul eq", "11"),
        ("2 mul 3 add eq", "12"),
        ("2 add 3 mul 4 add eq", "28"),
    ] {
        assert_eq!(shows(keys).await, shown, "{keys}");
    }
}

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

#[tokio::test]
async fn equals_finishes_the_sum_and_a_second_equals_does_nothing() {
    let mut pad = Keypad::open().await;
    pad.press("2 add 3 mul 4 eq eq eq").await;
    assert_eq!((pad.display(), pad.caption(), pad.pending()), ("14".into(), "2 + 3 × 4".into(), vec![]));
    assert_eq!(shows("eq").await, "0", "nothing to finish");
    assert_eq!(shows("7 eq").await, "7", "a typed number alone");
}

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
}

#[tokio::test]
async fn a_long_chain_keeps_the_caption_to_its_last_terms() {
    let mut pad = Keypad::open().await;
    pad.press("1 add 2 add 3 add 4 add 5 add 6 add 7 add").await;
    assert_eq!((pad.display(), pad.caption()), ("28".into(), "… 2 + 3 + 4 + 5 + 6 + 7 +".into()));
    pad.press("8 eq").await;
    assert_eq!((pad.display(), pad.caption()), ("36".into(), "… 3 + 4 + 5 + 6 + 7 + 8".into()));
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
        ("5 sub 2 negate eq", "7"),
        ("5 negate mul 3 eq", "-15"),
        ("3 sub 3 eq negate", "0"),
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
        ("2 add 5 0 percent eq", "2.5"),
        ("2 add 3 mul 5 0 percent eq", "3.5"),
        ("2 add 3 mul percent eq", "2.09"),
        ("0 dot 0 0 0 1 percent 5", "5"),
        ("5 0 percent dot 5", "0.5"),
        ("0 dot 0 0 0 1 percent mul 5 eq", "0.000005"),
    ] {
        assert_eq!(shows(keys).await, shown, "{keys}");
    }
}

#[tokio::test]
async fn results_in_exponent_form_are_ordinary_operands() {
    for (keys, shown) in [
        ("0 dot 0 0 0 0 0 0 1 mul 2 eq", "2e-7"),
        ("0 dot 0 0 0 0 0 0 1 add 1 eq", "1.0000001"),
        ("1 div 3 0 0 0 0 0 0 0 0 eq mul 3 eq", "1e-8"),
        ("9 9 9 9 9 9 9 9 9 mul 9 9 9 9 9 9 9 9 9 eq", "9.99999998e17"),
        ("1 0 0 0 0 0 0 0 0 mul 1 0 0 0 0 0 0 0 0 eq div 2 eq", "5e15"),
        ("0 dot 0 0 0 0 0 0 1 percent mul 1 0 0 0 0 0 0 0 0 eq", "0.1"),
    ] {
        assert_eq!(shows(keys).await, shown, "{keys}");
    }
}

#[tokio::test]
async fn division_by_zero_is_an_error_until_cleared_and_says_why() {
    for keys in ["8 div 0 eq", "8 div 0 add", "0 div 0 eq", "5 add 3 div 0 eq", "5 add 3 div 0 add"] {
        let mut pad = Keypad::open().await;
        pad.press(keys).await;
        assert_eq!((pad.display(), pad.caption()), ("Error".into(), "Can't divide by zero".into()), "{keys}");
        assert_eq!(pad.announced(), "Can't divide by zero", "{keys}: the client announces the reason");
        // keys with nothing to work on are ignored while the error shows
        pad.press("add sub mul div eq negate percent back").await;
        assert_eq!(pad.display(), "Error", "{keys}: ignored");
        assert_eq!(pad.clear_title(), "AC", "{keys}");
        pad.press("clear").await;
        assert_eq!(
            (pad.display(), pad.caption(), pad.pending()),
            ("0".into(), String::new(), vec![]),
            "{keys}"
        );
        assert_eq!(pad.announced(), "", "{keys}: the announcer is emptied, so the same error is read again");
        pad.press("4 add 4 eq").await;
        assert_eq!(pad.display(), "8", "{keys}: carry on");
    }
    assert_eq!(shows("0 div 5 eq").await, "0");
}

#[tokio::test]
async fn a_result_too_large_for_any_display_is_an_error_not_inf() {
    let mut pad = Keypad::open().await;
    pad.overflow().await;
    assert_eq!((pad.display(), pad.caption()), ("Error".into(), "Result too large".into()));
    pad.press("clear").await;
    assert_eq!(pad.display(), "0");
}

#[tokio::test]
async fn a_digit_after_an_error_starts_a_new_sum() {
    for (keys, shown) in [
        ("8 div 0 eq 5", "5"),
        ("8 div 0 eq dot", "0."),
        ("8 div 0 eq 5 add 4 eq", "9"),
        ("8 div 0 add 7 mul 2 eq", "14"),
    ] {
        assert_eq!(shows(keys).await, shown, "{keys}");
    }
    let mut pad = Keypad::open().await;
    pad.press("8 div 0 eq 5").await;
    assert_eq!((pad.caption(), pad.pending()), (String::new(), vec![]), "nothing of the failed sum is left");
}

#[tokio::test]
async fn backspace_removes_the_last_typed_character_only() {
    for (keys, shown) in [
        ("back", "0"),
        ("1 2 3 back", "12"),
        ("1 2 3 back back back back", "0"),
        ("1 dot 5 back", "1."),
        ("5 negate back", "0"),
        ("1 2 negate back", "-1"),
        ("1 2 add 3 eq back", "15"),
        ("1 2 add back 3 eq", "15"),
        ("1 2 add 3 4 back eq", "15"),
    ] {
        assert_eq!(shows(keys).await, shown, "{keys}");
    }
}

#[tokio::test]
async fn clear_clears_the_entry_first_and_everything_the_second_time() {
    let mut pad = Keypad::open().await;
    pad.press("5 add 3").await;
    assert_eq!((pad.clear_title(), pad.display()), ("C".into(), "3".into()), "something is being typed");
    pad.press("clear").await;
    assert_eq!((pad.clear_title(), pad.display(), pad.pending()), ("AC".into(), "0".into(), vec!["add"]));
    pad.press("4 eq").await;
    assert_eq!((pad.display(), pad.caption(), pad.clear_title()), ("9".into(), "5 + 4".into(), "AC".into()));
    pad.press("clear").await;
    assert_eq!((pad.display(), pad.caption()), ("0".into(), String::new()));
    pad.press("5 add 3 clear clear 2 eq").await;
    assert_eq!(pad.display(), "2", "no pending operator survives AC");
    assert_eq!(shows("2 add 3 mul 4 clear eq").await, "2", "`=` takes the cleared 0 as the operand");
    assert_eq!(shows("2 add 3 mul 4 clear 5 eq").await, "17");
}

#[tokio::test]
async fn each_window_has_a_calculator_of_its_own() {
    // the backend keeps nothing: two clients pressing keys in turn do not see each other's sums
    let mut one = Keypad::open().await;
    let rt = std::mem::replace(&mut one.rt, calculator().await);
    let mut two = Keypad { rt, screen: Value::Null };
    one.press("2 add").await;
    two.press("9 mul").await;
    one.press("3 eq").await;
    two.press("2 eq").await;
    assert_eq!((one.display(), two.display()), ("5".into(), "18".into()));
}

#[tokio::test]
async fn a_key_or_a_state_the_contract_does_not_allow_is_refused_before_the_handler_runs() {
    let rt = calculator().await;
    let good = json!({ "entry": "0", "value": 0, "numbers": [], "ops": [], "waiting": false, "fresh": true, "caption": "", "err": false });
    let mut long_entry = good.clone();
    long_entry["entry"] = json!("1".repeat(33));
    let mut bad_op = good.clone();
    bad_op["ops"] = json!(["^"]);
    for args in [
        json!({}),
        json!({ "key": "sqrt" }),
        json!({ "key": 7 }),
        json!({ "key": "7", "state": { "entry": "0" } }),
        json!({ "key": "7", "state": long_entry }),
        json!({ "key": "+", "state": bad_op }),
        json!({ "key": "7", "extra": true }),
    ] {
        let refused = call(&rt, args.clone()).await.unwrap_err();
        assert!(matches!(refused, PortError::Rpc { code: -32602, .. }), "{args}: {refused:?}");
    }
    assert_eq!(call(&rt, json!({ "key": "7", "state": good })).await.unwrap()["display"], "7");
}

#[tokio::test]
async fn a_state_that_makes_no_sense_is_an_error_on_the_display_never_a_failed_call() {
    // a client may send any state the contract allows; what makes no sense is the calculator's "Error", with a plain
    // sentence and no source position, and AC gets out of it
    let rt = calculator().await;
    let base = json!({ "entry": "0", "value": 0, "numbers": [], "ops": [], "waiting": false, "fresh": true, "caption": "", "err": false });
    let mut garbage_entry = base.clone();
    garbage_entry["entry"] = json!("abc");
    garbage_entry["fresh"] = json!(false);
    let mut uneven = base.clone();
    uneven["numbers"] = json!([1, 2, 3]);
    uneven["ops"] = json!(["+"]);
    let mut waiting_with_nothing = base.clone();
    waiting_with_nothing["waiting"] = json!(true);
    for (state, key) in
        [(garbage_entry, "+"), (garbage_entry_eq(), "="), (uneven, "="), (waiting_with_nothing, "+")]
    {
        let screen = call(&rt, json!({ "key": key, "state": state })).await.unwrap();
        let caption = screen["caption"].as_str().unwrap();
        assert!(!caption.contains(".luau") && !caption.contains("0x"), "{state}: {caption}");
        if screen["display"] == "Error" {
            let cleared = call(&rt, json!({ "key": "clear", "state": screen["state"] })).await.unwrap();
            assert_eq!(cleared["display"], "0");
        }
    }
}

fn garbage_entry_eq() -> Value {
    json!({ "entry": "1.2.3", "value": 0, "numbers": [1], "ops": ["+"], "waiting": false, "fresh": false, "caption": "", "err": false })
}
