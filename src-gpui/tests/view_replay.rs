//! The vendored GPUI's view cache and interaction redraws, as the interface
//! relies on them: a region stays replayable after the view around it was
//! replayed whole, and a hover that only fills an element draws no view again
//! where an element around it allows that.
use gpui_test::{
    AppContext, Context, Entity, InteractiveElement, IntoElement, Modifiers, ParentElement, Render,
    StatefulInteractiveElement, StyleRefinement, Styled, TestAppContext, VisualTestContext, Window,
    div, point, px, rgb,
};
use std::cell::Cell;
use std::rc::Rc;

#[derive(Clone, Default)]
struct Counts {
    outer: Rc<Cell<usize>>,
    inner: Rc<Cell<usize>>,
    clicks: Rc<Cell<usize>>,
    plain: Rc<Cell<usize>>,
}

fn bump(count: &Rc<Cell<usize>>) {
    count.set(count.get() + 1);
}

/// The innermost cached view, with something to click.
struct Inner {
    counts: Counts,
}
impl Render for Inner {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        bump(&self.counts.inner);
        let clicks = self.counts.clicks.clone();
        div()
            .id("inner-target")
            .size_full()
            .on_click(move |_, _, _| bump(&clicks))
    }
}

/// A cached view around another cached view, as the page region is around
/// the details and the resource cards.
struct Outer {
    counts: Counts,
    inner: Entity<Inner>,
}
impl Render for Outer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        bump(&self.counts.outer);
        div().size_full().child(
            self.inner
                .clone()
                .cached(StyleRefinement::default().w(px(200.)).h(px(100.))),
        )
    }
}

/// A view beside them that draws on its own, as the chart layer does.
struct Layer;
impl Render for Layer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

struct Root {
    outer: Entity<Outer>,
    layer: Entity<Layer>,
}
impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                self.outer
                    .clone()
                    .cached(StyleRefinement::default().w(px(300.)).h(px(200.))),
            )
            .child(self.layer.clone())
    }
}

#[test]
fn a_view_inside_a_replayed_view_still_replays() {
    let mut cx = TestAppContext::single();
    let counts = Counts::default();
    let window = cx.add_window({
        let counts = counts.clone();
        |_, cx| {
            let inner = cx.new(|_| Inner {
                counts: counts.clone(),
            });
            let outer = cx.new(|_| Outer { counts, inner });
            let layer = cx.new(|_| Layer);
            Root { outer, layer }
        }
    });
    cx.run_until_parked();
    let (outer, inner, layer) = window
        .update(&mut cx, |root, _, cx| {
            (
                root.outer.clone(),
                root.outer.read(cx).inner.clone(),
                root.layer.clone(),
            )
        })
        .unwrap();
    assert_eq!((counts.outer.get(), counts.inner.get()), (1, 1));

    for round in 1..=3 {
        // Frames that draw the layer alone replay the outer view whole: the
        // inner view is not visited in them.
        for _ in 0..round {
            layer.update(&mut cx, |_, cx| cx.notify());
            cx.run_until_parked();
        }
        assert_eq!(
            (counts.outer.get(), counts.inner.get()),
            (round, 1),
            "a frame of the layer rendered a cached view"
        );
        // The outer view then draws again without having changed, as a view
        // does around a region that changed or for an animation of its own.
        window
            .update(&mut cx, |_, window, _| {
                window.request_view_redraw(outer.entity_id())
            })
            .unwrap();
        cx.run_until_parked();
        assert_eq!(counts.outer.get(), round + 1);
        assert_eq!(
            counts.inner.get(),
            1,
            "the inner view rendered again after the view around it was replayed"
        );
    }

    // What the replays carried along is where it was drawn: the element
    // inside still takes its click.
    let mut visual = VisualTestContext::from_window(window.into(), &cx);
    visual.simulate_click(point(px(20.), px(20.)), Modifiers::default());
    assert_eq!(counts.clicks.get(), 1);
    assert_eq!(
        counts.inner.get(),
        3,
        "the press and the release each draw the view they are in again"
    );

    // A notification still renders the view it names.
    inner.update(&mut cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(counts.inner.get(), 4);
}

/// Elements under the pointer in turn. Inside an element that lets hover
/// fills change in place: one whose hover style is a background alone, and
/// one whose hover style also colors its text. Outside it: another whose
/// hover style is a background alone.
struct Hovered {
    counts: Counts,
}
impl Render for Hovered {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        bump(&self.counts.plain);
        let fill = |style: StyleRefinement| style.bg(rgb(0x334455));
        div()
            .size_full()
            .child(
                div()
                    .hover_fills_in_place()
                    .child(div().id("fill-only").w(px(100.)).h(px(40.)).hover(fill))
                    .child(
                        div()
                            .id("fill-and-text")
                            .w(px(100.))
                            .h(px(40.))
                            .hover(move |style| fill(style).text_color(rgb(0xffffff))),
                    ),
            )
            .child(
                div()
                    .id("fill-elsewhere")
                    .w(px(100.))
                    .h(px(40.))
                    .hover(fill),
            )
    }
}

#[test]
fn a_hover_fill_draws_no_view_again_where_fills_change_in_place() {
    let mut cx = TestAppContext::single();
    let counts = Counts::default();
    let window = cx.add_window({
        let counts = counts.clone();
        |_, _| Hovered { counts }
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), &cx);
    let renders = counts.plain.clone();
    let outside = point(px(10.), px(300.));
    let mut pointer_to = |y: f32| {
        visual.simulate_mouse_move(point(px(10.), px(y)), None, Modifiers::default());
    };
    let settled = renders.get();

    // Onto the element and off it: its quad is filled in the scene on screen.
    pointer_to(10.);
    pointer_to(f32::from(outside.y));
    pointer_to(10.);
    assert_eq!(
        renders.get(),
        settled,
        "a hover fill rendered the view again"
    );

    // A hover style that sets more than a background still draws the view.
    pointer_to(50.);
    assert!(
        renders.get() > settled,
        "a hover style with a text color did not draw its view again"
    );

    // So does a fill alone where nothing allows it in place: what counts on
    // a hover drawing its view again keeps working there.
    pointer_to(f32::from(outside.y));
    let settled = renders.get();
    pointer_to(90.);
    assert!(
        renders.get() > settled,
        "a hover fill outside the element that allows it did not draw its view again"
    );
}

/// An element with a hover fill that comes to lie under a resting pointer
/// when its view renders, nowhere that fills change in place.
struct Arriving {
    counts: Counts,
    shown: bool,
}
impl Render for Arriving {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        bump(&self.counts.plain);
        let root = div().size_full();
        if !self.shown {
            return root;
        }
        root.child(
            div()
                .id("arrives")
                .w(px(100.))
                .h(px(40.))
                .hover(|style| style.bg(rgb(0x334455))),
        )
    }
}

#[test]
fn an_element_painted_under_a_resting_pointer_is_left_like_any_other() {
    let mut cx = TestAppContext::single();
    let counts = Counts::default();
    let window = cx.add_window({
        let counts = counts.clone();
        |_, _| Arriving {
            counts,
            shown: false,
        }
    });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), &cx);
    visual.simulate_mouse_move(point(px(10.), px(10.)), None, Modifiers::default());
    // The element is laid out under the pointer, which did not move: it is
    // painted with its hover fill.
    window
        .update(&mut cx, |view, _, cx| {
            view.shown = true;
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    let settled = counts.plain.get();

    // The pointer leaves it: its view draws again, without the fill, although
    // the pointer's last move before this one was not onto the element.
    visual.simulate_mouse_move(point(px(10.), px(300.)), None, Modifiers::default());
    assert!(
        counts.plain.get() > settled,
        "an element painted hovered kept its fill after the pointer left it"
    );
}

/// A cached view inside an element that is hidden at times: hidden, the view
/// is prepainted and not painted.
struct Veiled {
    inner: Entity<Inner>,
    hidden: bool,
}
impl Render for Veiled {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let veil = div().size_full();
        let veil = if self.hidden { veil.invisible() } else { veil };
        veil.child(
            self.inner
                .clone()
                .cached(StyleRefinement::default().w(px(200.)).h(px(100.))),
        )
    }
}

#[test]
fn a_view_that_was_not_painted_in_the_last_frame_renders() {
    let mut cx = TestAppContext::single();
    let counts = Counts::default();
    let window = cx.add_window({
        let counts = counts.clone();
        |_, cx| Veiled {
            inner: cx.new(|_| Inner { counts }),
            hidden: true,
        }
    });
    cx.run_until_parked();
    let renders = counts.inner.clone();
    assert_eq!(renders.get(), 1);
    // The view around it draws again without having been notified, as for a
    // hover or a press, which replays the cached views inside it.
    let mut draw = |hidden: bool| {
        window
            .update(&mut cx, |view, window, cx| {
                view.hidden = hidden;
                window.request_view_redraw(cx.entity_id());
            })
            .unwrap();
        cx.run_until_parked();
    };

    draw(false);
    assert_eq!(
        renders.get(),
        2,
        "a view that was never painted replayed its paint"
    );
    draw(false);
    assert_eq!(renders.get(), 2, "a view drawn in the last frame rendered");
    // Hidden, its prepaint replays and nothing paints it.
    draw(true);
    assert_eq!(renders.get(), 2);
    draw(false);
    assert_eq!(
        renders.get(),
        3,
        "a view replayed a paint older than the last frame"
    );
}
