use super::*;
use crate::ui::node::{Action, Handler};
use serde_json::json;

fn codes(violations: &[Violation]) -> Vec<Code> {
    violations.iter().map(|v| v.code).collect()
}

fn errors(root: &Node) -> Vec<Violation> {
    validate(root).into_iter().filter(|v| v.severity == Severity::Error).collect()
}

fn only_error(root: &Node) -> Violation {
    let found = errors(root);
    assert_eq!(found.len(), 1, "{found:#?}");
    found.into_iter().next().unwrap_or_else(|| unreachable!())
}

fn window(child: Node) -> Node {
    Node::new("Group", "main").child(child)
}

fn button(key: &str) -> Node {
    Node::new("Button", key).prop("title", json!("Go")).handler("tap", Handler::Emit)
}

fn r(path: &str) -> Value {
    json!({ "$ref": path })
}

#[test]
fn a_well_formed_tree_has_no_findings() {
    let tree = window(
        Node::new("Stack", "_0")
            .prop("axis", json!("v"))
            .prop("spacing", json!(2))
            .prop("padding", json!({ "x": 2, "y": 1 }))
            .child(Node::new("Text", "title").prop("text", json!("Hello")).prop("style", json!("title1")))
            .child(button("go")),
    );
    assert_eq!(validate(&tree), vec![]);
}

#[test]
fn unknown_components_are_reported_with_their_path() {
    let v = only_error(&window(Node::new("Marquee", "m")));
    assert_eq!(v.code, Code::UnknownComponent);
    assert_eq!(v.path, "main/m");
    assert!(v.message.contains("Marquee"));
}

#[test]
fn unknown_props_name_the_prop_and_the_component() {
    let v = only_error(&window(Node::new("Text", "t").prop("text", json!("x")).prop("colour", json!("red"))));
    assert_eq!(v.code, Code::UnknownProp);
    assert_eq!(v.field.as_deref(), Some("props.colour"));
}

#[test]
fn raw_styling_is_not_in_the_vocabulary() {
    for style in ["color", "fontSize", "fontFamily", "css", "style_attr", "boxShadow"] {
        let v = only_error(&window(
            Node::new("Text", "t").prop("text", json!("x")).prop(style, json!("anything")),
        ));
        assert_eq!(v.code, Code::UnknownProp, "{style}");
    }
}

#[test]
fn wrong_prop_types_say_what_was_expected() {
    let cases = [
        ("Stack", "spacing", json!("big"), "a spacing"),
        ("Stack", "spacing", json!(9), "a spacing"),
        ("Stack", "axis", json!("diagonal"), "one of"),
        ("Text", "lines", json!(1.5), "an integer"),
        ("Text", "selectable", json!("yes"), "a boolean"),
        ("Text", "text", json!(3), "a string"),
        ("Stack", "padding", json!("lots"), "object"),
        ("Stack", "frame", json!("wide"), "object"),
    ];
    for (component, prop, value, expected) in cases {
        let mut node = Node::new(component, "n").prop(prop, value);
        if component == "Text" {
            node.props.entry("text".to_owned()).or_insert(json!("x"));
        }
        let v = only_error(&window(node));
        assert_eq!(v.code, Code::WrongType, "{component}.{prop}");
        assert!(v.message.contains(expected), "{component}.{prop}: {}", v.message);
    }
}

#[test]
fn enum_errors_list_the_allowed_values() {
    let v = only_error(&window(Node::new("Text", "t").prop("text", json!("x")).prop("style", json!("huge"))));
    assert!(v.message.contains("largeTitle") && v.message.contains("caption2"), "{}", v.message);
}

#[test]
fn missing_required_props() {
    let v = only_error(&window(Node::new("Text", "t")));
    assert_eq!(v.code, Code::MissingProp);
    assert_eq!(v.field.as_deref(), Some("props.text"));
    let v = only_error(&window(Node::new("Show", "s")));
    assert_eq!(v.field.as_deref(), Some("props.when"));
}

#[test]
fn a_button_needs_a_name() {
    let icon_only = Node::new("Button", "add").prop("icon", json!("plus")).handler("tap", Handler::Emit);
    let v = only_error(&window(icon_only.clone()));
    assert_eq!(v.code, Code::MissingName);
    assert!(v.message.contains("icon-only"), "{}", v.message);
    // a label fixes it, a title fixes it, custom content fixes it
    assert!(errors(&window(icon_only.clone().prop("label", json!("Add")))).is_empty());
    assert!(errors(&window(
        Node::new("Button", "b").prop("title", json!("Add")).handler("tap", Handler::Emit)
    ))
    .is_empty());
    assert!(errors(&window(icon_only.child(Node::new("Text", "t").prop("text", json!("Add"))))).is_empty());
    // an empty title is not a name
    assert_eq!(
        only_error(&window(Node::new("Button", "e").prop("title", json!("  ")))).code,
        Code::MissingName
    );
}

#[test]
fn a_ref_or_call_counts_as_a_name() {
    let node = Node::new("Button", "b").prop("title", r("client.title")).handler("tap", Handler::Emit);
    let tree = window(node);
    let ctx = Context { client: Some(["title".to_owned()].into()), ..Context::default() };
    assert!(is_valid(&validate_with(&tree, &ctx)));
}

#[test]
fn an_image_needs_alt_or_decorative() {
    let image = Node::new("Image", "i").prop("src", json!({ "asset": "logo" }));
    assert_eq!(only_error(&window(image.clone())).code, Code::MissingName);
    assert!(errors(&window(image.clone().prop("alt", json!("The logo")))).is_empty());
    assert!(errors(&window(image.prop("decorative", json!(true)))).is_empty());
}

#[test]
fn text_inputs_need_a_label_or_placeholder() {
    let field = Node::new("TextField", "q").prop("text", r("client.q"));
    let ctx = Context { client: Some(["q".to_owned()].into()), ..Context::default() };
    let bad = validate_with(&window(field.clone()), &ctx);
    assert_eq!(codes(&bad), [Code::MissingName]);
    assert!(is_valid(&validate_with(&window(field.clone().prop("placeholder", json!("Search"))), &ctx)));
    assert!(is_valid(&validate_with(&window(field.prop("label", json!("Search"))), &ctx)));
}

#[test]
fn bindings_must_be_refs() {
    let field = |text: Value| Node::new("TextEditor", "body").prop("text", text).prop("label", json!("Note"));
    let v = only_error(&window(field(json!("literal"))));
    assert_eq!(v.code, Code::BindingNotRef);
    let v = only_error(&window(field(json!({ "$fn": "upper", "args": [r("client.a")] }))));
    assert_eq!(v.code, Code::BindingNotRef);
    assert!(errors(&window(field(r("src.s_note.body")))).is_empty());
    assert!(errors(&window(field(r("state.draft")))).is_empty());
}

#[test]
fn bindings_cannot_write_read_only_namespaces() {
    let field = Node::new("TextEditor", "body").prop("text", r("env.locale")).prop("label", json!("Note"));
    assert_eq!(only_error(&window(field)).code, Code::BindingNotWritable);
    let toggle = Node::new("Toggle", "t").prop("on", r("route.flag")).prop("title", json!("x"));
    let found = errors(&window(
        Node::new("NavigationStack", "nav")
            .slot("root", vec![toggle.clone()])
            .slot("dest:note", vec![toggle]),
    ));
    assert!(codes(&found).contains(&Code::BindingNotWritable));
}

#[test]
fn refs_in_ordinary_props_may_point_anywhere_readable() {
    let tree = window(Node::new("Text", "t").prop("text", r("env.locale")));
    assert!(errors(&tree).is_empty());
}

#[test]
fn duplicate_sibling_keys() {
    let tree = window(
        Node::new("Stack", "s")
            .child(Node::new("Text", "a").prop("text", json!("1")))
            .child(Node::new("Text", "a").prop("text", json!("2"))),
    );
    let v = only_error(&tree);
    assert_eq!(v.code, Code::DuplicateKey);
    assert_eq!(v.path, "main/s/a");
}

#[test]
fn the_same_key_in_different_slots_is_fine_for_keys() {
    let tree = window(
        Node::new("Show", "s")
            .prop("when", json!(true))
            .child(Node::new("Text", "a").prop("text", json!("1")))
            .slot("otherwise", vec![Node::new("Text", "a").prop("text", json!("2"))]),
    );
    assert_eq!(errors(&tree), vec![], "the two arms of a Show are never mounted together");
    // the same key in two slots of an ordinary container shares a derived test id (slots do not rename)
    let clash = window(
        Node::new("Screen", "s")
            .child(Node::new("Text", "a").prop("text", json!("1")))
            .slot("toolbar.trailing", vec![Node::new("Text", "a").prop("text", json!("2"))]),
    );
    assert_eq!(only_error(&clash).code, Code::DuplicateTestId);
}

#[test]
fn interactive_nodes_need_explicit_keys() {
    let v = only_error(&window(Node::new("Button", "_0").prop("title", json!("Go"))));
    assert_eq!(v.code, Code::InteractiveNeedsKey);
    // a node with a handler is interactive even if the component is not
    let v =
        only_error(&window(Node::new("Row", "_1").prop("title", json!("r")).handler("tap", Handler::Emit)));
    assert_eq!(v.code, Code::InteractiveNeedsKey);
    // auto keys are fine on static content
    assert!(errors(&window(Node::new("Text", "_0").prop("text", json!("hi")))).is_empty());
}

#[test]
fn invalid_keys() {
    for key in ["", "a/b", "@slot", "has space"] {
        let found = errors(&window(Node::new("Spacer", key)));
        assert!(codes(&found).contains(&Code::InvalidKey), "`{key}`");
    }
}

#[test]
fn slots_are_checked() {
    let v = only_error(&window(Node::new("Group", "g").slot("sidebar", vec![Node::new("Spacer", "s")])));
    assert_eq!(v.code, Code::UnknownSlot);
    let two = vec![Node::new("Spacer", "a"), Node::new("Spacer", "b")];
    let v = only_error(&window(
        Node::new("TextField", "q").prop("label", json!("q")).prop("text", r("client.q")).slot("prefix", two),
    ));
    assert_eq!(v.code, Code::SlotTooLarge);
    let v = only_error(&window(Node::new("ForEach", "f").prop("source", json!("s"))));
    assert_eq!(v.code, Code::MissingSlot);
    assert_eq!(v.field.as_deref(), Some("slots.row"));
}

#[test]
fn prefix_slots_match_dynamic_names() {
    let stack = Node::new("NavigationStack", "nav")
        .slot("root", vec![Node::new("Spacer", "r")])
        .slot("dest:note", vec![Node::new("Text", "t").prop("text", r("route.id"))]);
    assert_eq!(errors(&window(stack)), vec![]);
}

#[test]
fn children_rules() {
    let v =
        only_error(&window(Node::new("Text", "t").prop("text", json!("x")).child(Node::new("Spacer", "s"))));
    assert_eq!(v.code, Code::ChildrenNotAllowed);
    let v = only_error(&window(
        Node::new("Toggle", "t")
            .prop("on", r("client.on"))
            .prop("title", json!("x"))
            .child(Node::new("Spacer", "a"))
            .child(Node::new("Spacer", "b")),
    ));
    assert_eq!(v.code, Code::ChildrenNotAllowed);
}

#[test]
fn events_must_be_declared_by_the_component() {
    let v =
        only_error(&window(Node::new("Text", "t").prop("text", json!("x")).handler("tap", Handler::Emit)));
    assert_eq!(v.code, Code::UnknownEvent);
    assert_eq!(v.field.as_deref(), Some("on.tap"));
    // common events are accepted everywhere
    assert!(errors(&window(
        Node::new("Text", "t").prop("text", json!("x")).handler("appear", Handler::Emit)
    ))
    .is_empty());
}

#[test]
fn actions_are_checked() {
    let set =
        |target: &str| Handler::Actions(vec![Action::Set { target: target.into(), value: json!(true) }]);
    let ok = window(button("go").handler("tap", set("client.sheet")));
    assert!(errors(&ok).is_empty());
    let read_only = window(button("go").handler("tap", set("env.locale")));
    assert_eq!(only_error(&read_only).code, Code::BadHandler);
    let junk = window(button("go").handler("tap", set("nonsense")));
    assert_eq!(only_error(&junk).code, Code::BadHandler);
    let push =
        Action::NavPush { stack: None, route: json!({ "id": 7 }).as_object().cloned().unwrap_or_default() };
    let nameless = window(button("go").handler("tap", Handler::Actions(vec![push])));
    assert_eq!(only_error(&nameless).code, Code::BadHandler);
}

#[test]
fn expressions_use_the_closed_function_set() {
    let bad_fn = window(Node::new("Text", "t").prop("text", json!({ "$fn": "eval", "args": ["1+1"] })));
    assert_eq!(only_error(&bad_fn).code, Code::BadExpression);
    let bad_arity = window(Node::new("Text", "t").prop("text", json!({ "$fn": "if", "args": [true, "a"] })));
    assert_eq!(only_error(&bad_arity).code, Code::BadExpression);
    let bad_ref = window(Node::new("Text", "t").prop("text", r("nowhere.x")));
    assert_eq!(only_error(&bad_ref).code, Code::BadExpression);
    let nested = window(Node::new("Text", "t").prop(
        "text",
        json!({ "$fn": "concat", "args": [r("env.locale"), { "$fn": "upper", "args": ["x"] }] }),
    ));
    assert!(errors(&nested).is_empty());
}

#[test]
fn row_refs_only_exist_in_row_templates() {
    let outside = window(Node::new("Text", "t").prop("text", r("row.title")));
    assert_eq!(only_error(&outside).code, Code::RefOutOfScope);
    let inside = window(
        Node::new("ForEach", "f")
            .prop("source", json!("s"))
            .slot("row", vec![Node::new("Text", "_0").prop("text", r("row.title"))]),
    );
    assert!(errors(&inside).is_empty());
    let list = window(
        Node::new("List", "l")
            .prop("source", json!("s"))
            .slot("row", vec![Node::new("Row", "_0").prop("title", r("row.title"))]),
    );
    assert!(errors(&list).is_empty());
}

#[test]
fn route_refs_only_exist_in_destinations() {
    let outside = window(Node::new("Text", "t").prop("text", r("route.id")));
    assert_eq!(only_error(&outside).code, Code::RefOutOfScope);
}

#[test]
fn declarations_are_checked_when_supplied() {
    let tree = window(
        Node::new("Stack", "s")
            .child(Node::new("Text", "a").prop("text", r("client.nope")))
            .child(Node::new("Text", "b").prop("text", r("src.gone.field")))
            .child(
                Node::new("ForEach", "f")
                    .prop("source", json!("missing"))
                    .slot("row", vec![Node::new("Spacer", "_0")]),
            ),
    );
    assert!(errors(&tree).is_empty(), "no declarations, no checks");
    let decls = ViewDecls::default();
    let found = validate_with(&tree, &Context::from_decls(&decls));
    assert_eq!(codes(&found), [Code::UndeclaredRef, Code::UndeclaredRef, Code::UndeclaredRef]);
}

#[test]
fn declared_names_pass() {
    let mut decls = ViewDecls::default();
    decls.client.insert("search".into(), crate::ui::ClientDecl::default());
    decls.sources.insert(
        "s_notes".into(),
        crate::ui::SourceDecl::Rows {
            key: "id".into(),
            page: None,
            debounce: None,
            total: None,
            params: serde_json::Map::new(),
        },
    );
    let tree = window(
        Node::new("List", "l")
            .prop("source", json!("s_notes"))
            .slot("row", vec![Node::new("Row", "_0").prop("title", r("row.title"))])
            .prop("selection", r("client.search")),
    );
    assert_eq!(validate_with(&tree, &Context::from_decls(&decls)), vec![]);
}

#[test]
fn derived_test_ids_must_be_unique() {
    let tree = window(
        Node::new("Stack", "s")
            .child(Node::new("Group", "_0").child(button("save")))
            .child(Node::new("Group", "_1").child(button("save"))),
    );
    let v = only_error(&tree);
    assert_eq!(v.code, Code::DuplicateTestId);
    assert!(v.message.contains("testid"));
    // an explicit testid resolves it
    let fixed = window(
        Node::new("Stack", "s")
            .child(Node::new("Group", "_0").child(button("save")))
            .child(Node::new("Group", "_1").child(button("save").prop("testid", json!("other-save")))),
    );
    assert!(errors(&fixed).is_empty());
}

#[test]
fn drag_without_a_context_menu_is_a_warning() {
    let draggable = Node::new("Row", "card")
        .prop("title", json!("Card"))
        .prop("drag", json!({ "type": "task", "payload": 1 }));
    let found = validate(&window(draggable.clone()));
    assert_eq!(codes(&found), [Code::DragWithoutAlternative]);
    assert_eq!(found[0].severity, Severity::Warning);
    assert!(is_valid(&found));
    let with_menu =
        draggable.slot("contextMenu", vec![Node::new("Button", "move").prop("title", json!("Move to..."))]);
    assert_eq!(validate(&window(with_menu)), vec![]);
}

#[test]
fn newer_components_need_a_fallback_for_older_clients() {
    let kit = |minor| Context { client_kit: Some(KitVersion::new(1, minor)), ..Context::default() };
    // every catalogue entry is `since 1.0` today, so a 1.0 client supports all of them
    assert!(validate_with(&window(Node::new("Spacer", "s")), &kit(0)).is_empty());
    // a client on an older major does not
    let ancient = Context { client_kit: Some(KitVersion::new(0, 9)), ..Context::default() };
    let found = validate_with(&window(Node::new("Spacer", "s")), &ancient);
    assert!(found.iter().any(|v| v.code == Code::NewerThanClient && v.severity == Severity::Warning));
}

#[test]
fn placeholder_components_accept_any_prop_and_slot() {
    let tree = window(
        Node::new("CodeEditor", "ed")
            .prop("language", json!("rust"))
            .slot("gutter", vec![Node::new("Spacer", "g")]),
    );
    assert!(errors(&tree).is_empty());
}

#[test]
fn nested_object_props_are_checked() {
    let screen = |searchable: Value| Node::new("Screen", "s").prop("searchable", searchable);
    let ok = screen(json!({ "text": r("client.search"), "placeholder": "Search", "sync": "debounce:250" }));
    assert!(errors(&window(ok)).is_empty());
    let literal_text = screen(json!({ "text": "x" }));
    assert!(errors(&window(literal_text))
        .iter()
        .any(|v| v.field.as_deref() == Some("props.searchable/text")));
    let unknown_field = screen(json!({ "text": r("client.search"), "bogus": 1 }));
    assert!(errors(&window(unknown_field)).iter().any(|v| v.message.contains("bogus")));
    let missing = screen(json!({ "placeholder": "x" }));
    assert!(errors(&window(missing)).iter().any(|v| v.message.contains("missing `text`")));
}

#[test]
fn list_element_types_are_checked() {
    let picker = |options: Value| {
        Node::new("Picker", "p")
            .prop("selection", r("client.m"))
            .prop("label", json!("Mode"))
            .prop("options", options)
    };
    let ok = picker(json!([{ "value": "edit", "label": "Edit" }]));
    assert!(errors(&window(ok)).is_empty());
    let bad = picker(json!([{ "value": "edit" }]));
    assert!(errors(&window(bad)).iter().any(|v| v.message.contains("missing `label`")));
    let as_source = picker(json!("s_modes"));
    assert!(errors(&window(as_source)).is_empty());
}

#[test]
fn escaped_literals_are_checked_as_literals() {
    let tree = window(Node::new("Text", "t").prop("text", json!({ "$lit": "{ \"$ref\": \"x\" }" })));
    assert!(errors(&tree).is_empty());
}

#[test]
fn depth_and_size_limits() {
    let mut deep = Node::new("Spacer", "leaf");
    for i in 0..limits::MAX_DEPTH + 2 {
        deep = Node::new("Group", format!("g{i}")).child(deep);
    }
    assert!(codes(&errors(&deep)).contains(&Code::TooDeep));

    let wide = Node {
        children: (0..limits::MAX_NODES + 5).map(|i| Node::new("Spacer", format!("n{i}"))).collect(),
        ..Node::new("Group", "main")
    };
    assert!(codes(&errors(&wide)).contains(&Code::TooManyNodes));
}

#[test]
fn violations_print_with_their_path_and_field() {
    let v = only_error(&window(Node::new("Text", "t").prop("text", json!("x")).prop("lines", json!("two"))));
    let text = v.to_string();
    assert!(text.starts_with("error: main/t props.lines: "), "{text}");
}

#[test]
fn every_tier_one_component_validates_when_built_from_its_own_spec() {
    use crate::ui::components::CATALOGUE;
    use crate::ui::spec::{PropType, Tier};
    fn sample(ty: &PropType) -> Value {
        match ty {
            PropType::Str | PropType::Source => json!("x"),
            PropType::Number => json!(1.5),
            PropType::Int | PropType::Spacing | PropType::Insets => json!(2),
            PropType::Bool => json!(true),
            PropType::Json => json!("v"),
            PropType::Enum(def) => json!(def.values[0]),
            PropType::Frame => json!("fill"),
            PropType::List(inner) => json!([sample(inner)]),
            PropType::Object(def) => Value::Object(
                def.fields
                    .iter()
                    .filter(|f| f.required)
                    .map(|f| {
                        (f.name.to_owned(), if f.binding.is_some() { r("client.v") } else { sample(&f.ty) })
                    })
                    .collect(),
            ),
            PropType::Union(options) => sample(&options[0]),
        }
    }
    for spec in CATALOGUE.iter().filter(|c| c.tier == Tier::One) {
        let mut node = Node::new(spec.name, "n");
        for prop in spec.props {
            node.props.insert(
                prop.name.to_owned(),
                if prop.binding.is_some() { r("client.v") } else { sample(&prop.ty) },
            );
        }
        node.props.entry("label".to_owned()).or_insert(json!("Name"));
        for event in spec.events {
            node.on.insert(event.name.to_owned(), Handler::Emit);
        }
        for slot in spec.slots.iter().filter(|s| !s.prefix) {
            let content = Node::new("Text", "_0").prop("text", json!("t"));
            node.slots.insert(slot.name.to_owned(), vec![content]);
        }
        let mut decls = ViewDecls::default();
        decls.client.insert("v".into(), crate::ui::ClientDecl::default());
        decls.sources.insert(
            "x".into(),
            crate::ui::SourceDecl::Rows {
                key: "id".into(),
                page: None,
                debounce: None,
                total: None,
                params: serde_json::Map::new(),
            },
        );
        let tree = Node::new("Group", "main").child(node);
        let found = errors_with(&tree, &Context::from_decls(&decls));
        // Required slots only (the generated children above fill every slot, which can exceed `max`)
        let relevant: Vec<_> = found
            .iter()
            .filter(|v| !matches!(v.code, Code::ChildrenNotAllowed | Code::RefOutOfScope))
            .collect();
        assert!(relevant.is_empty(), "{}: {relevant:#?}", spec.name);
    }
}

fn errors_with(root: &Node, context: &Context) -> Vec<Violation> {
    validate_with(root, context).into_iter().filter(|v| v.severity == Severity::Error).collect()
}
