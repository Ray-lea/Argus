//! Text reaches assistive technology through the vendored GPUI's
//! accessibility tree (issue #115): text without an element id names the
//! control around it or becomes a label of its own, hidden text stays out,
//! and a frame that replays a cached view keeps that view's nodes.
use gpui_test::{
    AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Role,
    StatefulInteractiveElement, StyleRefinement, Styled, TestAppContext, Window, WindowHandle, div,
    px,
};
use serde_json::Value;
use std::cell::Cell;
use std::rc::Rc;

/// A cached view of text in the places the interface puts it.
struct Labels {
    renders: Rc<Cell<usize>>,
}
impl Render for Labels {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        div()
            .size_full()
            .child("Plain text")
            .child(div().child("Nested text"))
            // A control without a name of its own.
            .child(div().id("unnamed").role(Role::Button).child("Open"))
            // A named control whose text says more than its name.
            .child(
                div()
                    .id("named")
                    .role(Role::Button)
                    .aria_label("CPU")
                    .child("CPU")
                    .child(div().child("12%")),
            )
            // A combo box that lays out every option and shows one.
            .child(
                div()
                    .id("combo")
                    .role(Role::ComboBox)
                    .aria_label("Theme")
                    .child(div().child("Dark"))
                    .child(div().invisible().child("Light")),
            )
            .child(div().invisible().child("Hidden"))
    }
}

/// A view beside it that draws on its own, as the chart layer does.
struct Layer;
impl Render for Layer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

struct Root {
    labels: Entity<Labels>,
    layer: Entity<Layer>,
}
impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                self.labels
                    .clone()
                    .cached(StyleRefinement::default().w(px(300.)).h(px(200.))),
            )
            .child(self.layer.clone())
    }
}

/// The `aria` part of every node in the last tree built for the window.
fn nodes(cx: &mut TestAppContext, window: WindowHandle<Root>) -> Vec<Value> {
    let json = window
        .update(cx, |_, window, _| window.debug_a11y_tree_json())
        .unwrap()
        .expect("a tree was built");
    let tree: Value = serde_json::from_str(&json).unwrap();
    tree["nodes"]
        .as_object()
        .unwrap()
        .values()
        .map(|node| node["aria"].clone())
        .collect()
}

fn find<'a>(nodes: &'a [Value], role: &str, key: &str, text: &str) -> Option<&'a Value> {
    nodes
        .iter()
        .find(|node| node["role"] == role && node[key] == text)
}

fn mentions(nodes: &[Value], text: &str) -> bool {
    nodes.iter().any(|node| {
        ["label", "value", "description"].iter().any(|key| {
            node[*key]
                .as_str()
                .is_some_and(|value| value.contains(text))
        })
    })
}

#[test]
fn text_reaches_the_tree_and_stays_through_replays() {
    let mut cx = TestAppContext::single();
    let renders = Rc::new(Cell::new(0));
    let window = cx.add_window({
        let renders = renders.clone();
        |_, cx| Root {
            labels: cx.new(|_| Labels { renders }),
            layer: cx.new(|_| Layer),
        }
    });
    cx.run_until_parked();
    let layer = window
        .update(&mut cx, |root, _, _| root.layer.clone())
        .unwrap();
    window
        .update(&mut cx, |_, window, _| window.activate_a11y_for_test())
        .unwrap();
    cx.run_until_parked();

    let check = |nodes: &[Value], when: &str| {
        for text in ["Plain text", "Nested text"] {
            assert!(
                find(nodes, "Label", "value", text).is_some(),
                "{when}: {text:?} is not a label: {nodes:#?}"
            );
        }
        assert!(
            find(nodes, "Button", "label", "Open").is_some(),
            "{when}: the button is not named by its text: {nodes:#?}"
        );
        let named = find(nodes, "Button", "label", "CPU")
            .unwrap_or_else(|| panic!("{when}: the named button is gone: {nodes:#?}"));
        assert_eq!(
            named["description"], "12%",
            "{when}: the text beyond the button's name does not describe it"
        );
        let combo = find(nodes, "ComboBox", "label", "Theme")
            .unwrap_or_else(|| panic!("{when}: the combo box is gone: {nodes:#?}"));
        assert_eq!(combo["value"], "Dark", "{when}: the combo box's value");
        for text in ["Light", "Hidden"] {
            assert!(
                !mentions(nodes, text),
                "{when}: hidden {text:?} was reported"
            );
        }
        assert!(
            !nodes
                .iter()
                .any(|node| node["role"] == "Label" && node["value"] == "Open"),
            "{when}: the button's text is also a label of its own"
        );
    };
    check(&nodes(&mut cx, window), "after activation");
    let rendered = renders.get();

    // A frame of the layer alone replays the cached view: its nodes must not
    // leave the tree (upstream sent a tree without them).
    layer.update(&mut cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(
        renders.get(),
        rendered,
        "the layer's frame rendered the cached view instead of replaying it"
    );
    check(&nodes(&mut cx, window), "after a replaying frame");

    // A complete frame follows, which renders the cached view again.
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(300));
    cx.run_until_parked();
    assert!(
        renders.get() > rendered,
        "no complete frame followed the replaying one"
    );
    check(&nodes(&mut cx, window), "after the complete frame");
}
