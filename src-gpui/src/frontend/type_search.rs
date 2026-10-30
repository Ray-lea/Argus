//! Typing on the process list starts a search (`Desktop::key_down`), and an
//! input method's typing does too, from the first key.
//!
//! An input method composes only for a window that offers it an input
//! context, and GPUI's Windows platform takes the context away after any frame
//! without a focused text input (`update_ime_enabled`). The method sees a key
//! when it is read from the queue, before the application does, so focusing
//! the search box in a key handler is too late for that key: it arrived raw
//! and the composition began only with the next one. While the list has the
//! keyboard, [`SearchComposer`] stands in as its text input: the window keeps
//! its context, the method takes the first key, and the composition goes to
//! the search box, which takes focus as it starts. Plain keys still reach the
//! list (letters and digits that no method takes start the search there), and
//! text a method sends outside a composition (punctuation it passes on) starts
//! no search, as in the interface's process list (an `EditContext`).
use super::*;
use std::ops::Range;

/// What the list does with text an input method sends it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Route {
    /// Nothing: text outside a composition, or an empty one that never started.
    Ignore,
    /// A composition begins: the search box takes focus and the text.
    Start,
    /// The search box holds a composition the method is still working on.
    Forward,
}

/// Marked text: the composition, as the method has it so far.
pub(super) fn route_marked(composing: bool, text: &str) -> Route {
    if composing {
        Route::Forward
    } else if text.is_empty() {
        Route::Ignore
    } else {
        Route::Start
    }
}

/// Committed text: only a composition's commit goes to the search box.
pub(super) fn route_commit(composing: bool) -> Route {
    if composing {
        Route::Forward
    } else {
        Route::Ignore
    }
}

/// Whether the list stands in as the text input: on the process list, with no
/// menu or dialog open and the search box not focused (it is its own input
/// then). A focused text input painted later replaces this one anyway.
pub(super) fn armed(page: &str, overlay: bool, searching: bool) -> bool {
    page == "processes" && !overlay && !searching
}

/// Where a composition starts, in UTF-16 units: after the query, where a typed
/// letter goes too.
pub(super) fn end_of(value: &str) -> Range<usize> {
    let end = value.encode_utf16().count();
    end..end
}

/// A composition the list started, after a change to the search box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Settled {
    Composing,
    /// Text was committed: typing carries on in the box.
    Committed,
    /// Nothing was committed: the keyboard goes back where it was.
    Cancelled,
}

pub(super) fn settled(before: &str, value: &str, marked: bool) -> Settled {
    if marked {
        Settled::Composing
    } else if value == before {
        Settled::Cancelled
    } else {
        Settled::Committed
    }
}

/// A composition [`SearchComposer`] moved into the search box.
pub(super) struct ComposedSearch {
    /// The query before it, which a cancelled composition leaves.
    before: String,
    /// What had the keyboard: a cancelled composition gives it back.
    back: Option<FocusHandle>,
}

/// The process list's text input, painted while [`armed`].
pub(super) struct SearchComposer {
    desktop: WeakEntity<Desktop>,
    search: Entity<InputState>,
}

impl SearchComposer {
    /// The search box's own input handler, where it was last painted.
    fn target(&self, cx: &App) -> ElementInputHandler<InputState> {
        let bounds = self.search.read(cx).text_bounds().unwrap_or_default();
        ElementInputHandler::new(bounds, self.search.clone())
    }

    fn composing(&self, window: &mut Window, cx: &mut App) -> bool {
        self.search
            .update(cx, |state, cx| state.marked_text_range(window, cx))
            .is_some()
    }
}

impl InputHandler for SearchComposer {
    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<UTF16Selection> {
        if self.composing(window, cx) {
            return self
                .target(cx)
                .selected_text_range(ignore_disabled_input, window, cx);
        }
        // Asked as a composition starts (to place the method's window): the
        // composition will follow the query, whatever the box has selected.
        Some(UTF16Selection {
            range: end_of(&self.search.read(cx).value()),
            reversed: false,
        })
    }

    fn marked_text_range(&mut self, window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        self.target(cx).marked_text_range(window, cx)
    }

    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<String> {
        self.target(cx)
            .text_for_range(range_utf16, adjusted_range, window, cx)
    }

    fn replace_text_in_range(
        &mut self,
        replacement_range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        if route_commit(self.composing(window, cx)) == Route::Forward {
            self.target(cx)
                .replace_text_in_range(replacement_range, text, window, cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let route = route_marked(self.composing(window, cx), new_text);
        if route == Route::Ignore {
            return;
        }
        let started = (route == Route::Start)
            .then(|| {
                self.desktop
                    .update(cx, |desktop, cx| desktop.begin_composed_search(window, cx))
                    .ok()
            })
            .flatten();
        self.target(cx).replace_and_mark_text_in_range(
            range_utf16,
            new_text,
            new_selected_range,
            window,
            cx,
        );
        // Recorded after the text is marked: a change seen before that is not
        // this composition's end.
        if let Some(started) = started {
            let _ = self
                .desktop
                .update(cx, |desktop, _| desktop.composing = Some(started));
        }
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut App) {
        self.target(cx).unmark_text(window, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        self.target(cx).bounds_for_range(range_utf16, window, cx)
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<usize> {
        self.target(cx).character_index_for_point(point, window, cx)
    }
}

impl Desktop {
    /// An element registering [`SearchComposer`] with the platform, first in
    /// the frame so that a focused text input painted after it takes over.
    pub(super) fn search_composer(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let searching = self.search.read(cx).focus_handle(cx).is_focused(window);
        if !armed(
            &self.page,
            self.popup.is_some() || self.modal.is_some(),
            searching,
        ) {
            return None;
        }
        // An input handler belongs to a focused element: with the keyboard on
        // nothing (the window has just opened, a menu has closed), the page
        // takes it, as a click on an empty spot does.
        if window.focused(cx).is_none() {
            let focus = self.focus.clone();
            window.defer(cx, move |window, cx| {
                if window.focused(cx).is_none() {
                    focus.focus(window, cx);
                }
            });
        }
        let composer = SearchComposer {
            desktop: cx.entity().downgrade(),
            search: self.search.clone(),
        };
        Some(
            canvas(
                |_, _, _| {},
                move |_, _, window, cx| {
                    if let Some(holder) = window.focused(cx) {
                        window.handle_input(&holder, composer, cx);
                    }
                },
            )
            .absolute()
            .size_0()
            .into_any_element(),
        )
    }

    /// A composition starts on the list: the search box takes the keyboard
    /// with its caret after the query, as a typed letter would leave it.
    fn begin_composed_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ComposedSearch {
        let started = ComposedSearch {
            before: self.query.clone(),
            back: window.focused(cx),
        };
        let query = self.query.clone();
        self.search.update(cx, |state, cx| {
            state.set_value(query, window, cx);
            state.focus(window, cx);
        });
        cx.notify();
        started
    }

    /// Whether the search box holds text an input method is still composing.
    fn search_composing(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.search
            .update(cx, |state, cx| state.marked_text_range(window, cx))
            .is_some()
    }

    /// After any change to the search box: a composition the list started has
    /// ended once nothing is marked. Cancelled, it leaves the box as it was
    /// and gives the keyboard back, unless something else has taken it since.
    pub(super) fn composition_settled(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(started) = &self.composing else {
            return;
        };
        let marked = self.search_composing(window, cx);
        let value = self.search.read(cx).value();
        match settled(&started.before, &value, marked) {
            Settled::Composing => {}
            Settled::Committed => self.composing = None,
            Settled::Cancelled => {
                let back = self.composing.take().and_then(|started| started.back);
                if self.search.read(cx).focus_handle(cx).is_focused(window) {
                    back.unwrap_or_else(|| self.focus.clone()).focus(window, cx);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // Not a glob: GPUI's `test` attribute would replace the standard one.
    use super::{Route, Settled, armed, end_of, route_commit, route_marked, settled};

    #[test]
    fn a_composition_starts_the_search() {
        // The first marked text of a composition starts the search, from the list.
        assert_eq!(route_marked(false, "n"), Route::Start);
        assert_eq!(route_marked(false, "ni"), Route::Start);
        // An empty update outside a composition is not one.
        assert_eq!(route_marked(false, ""), Route::Ignore);
        // Once the box holds the composition, every update goes there, the
        // empty one that cancels it included.
        assert_eq!(route_marked(true, "nih"), Route::Forward);
        assert_eq!(route_marked(true, ""), Route::Forward);
    }

    #[test]
    fn only_a_composition_commits_into_the_search() {
        assert_eq!(route_commit(true), Route::Forward);
        // Outside a composition: punctuation a method passes on, or characters
        // of keys the list left alone (a held key, AltGr).
        assert_eq!(route_commit(false), Route::Ignore);
    }

    #[test]
    fn the_list_stands_in_only_where_typing_starts_a_search() {
        assert!(armed("processes", false, false));
        assert!(!armed("processes", true, false));
        assert!(!armed("processes", false, true));
        for page in [
            "performance",
            "services",
            "startup",
            "network",
            "history",
            "settings",
        ] {
            assert!(!armed(page, false, false), "{page}");
        }
    }

    #[test]
    fn a_composition_follows_the_query() {
        assert_eq!(end_of(""), 0..0);
        assert_eq!(end_of("chrome"), 6..6);
        // UTF-16 units, as the platform counts them.
        assert_eq!(end_of("微信"), 2..2);
        assert_eq!(end_of("a😀"), 3..3);
    }

    #[test]
    fn a_composition_ends_committed_or_cancelled() {
        assert_eq!(settled("ab", "abni", true), Settled::Composing);
        assert_eq!(settled("ab", "ab你", false), Settled::Committed);
        assert_eq!(settled("", "你", false), Settled::Committed);
        assert_eq!(settled("ab", "ab", false), Settled::Cancelled);
        assert_eq!(settled("", "", false), Settled::Cancelled);
    }
}
