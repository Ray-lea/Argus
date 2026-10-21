// Where keyboard focus rests on the processes page. Input methods only
// compose in an editable element, and the process list (an EditContext, see
// process-search.ts) is the one there. With focus on nothing or on a button,
// Chromium hands the input method a read-only document: Microsoft Pinyin and
// the like still take the first key, show it and drop it, and the page never
// sees it. So focus that would rest there goes back to the list. The decision
// has no DOM or Solid in it; ui/tests runs it directly.

/** What has focus, as far as the list is concerned. */
export type FocusHolder = "list" | "text" | "control" | "nothing";

export interface FocusState {
  holder: FocusHolder;
  /** The window has focus, not another application. */
  windowFocused: boolean;
  /** A dialog, menu or list box is open: it places focus itself. */
  popupOpen: boolean;
  /** A pointer is still down: a text selection or a drag may be under way. */
  pointerDown: boolean;
  /** Text is selected (in the details panel, say), to be copied. */
  selecting: boolean;
  /** Checked right after a click: a button the click focused gives it back. */
  afterClick: boolean;
  /** The page has just opened: hand focus from navigation to its content. */
  enteringPage: boolean;
}

/** Text fields keep focus; buttons keep it during keyboard navigation within the page. */
export function reclaimsFocus(s: FocusState): boolean {
  if (!s.windowFocused || s.popupOpen || s.pointerDown || s.selecting) return false;
  if (s.holder === "nothing") return true;
  return s.holder === "control" && (s.afterClick || s.enteringPage);
}

function holderOf(el: Element | null, list: HTMLElement): FocusHolder {
  if (!el || el === document.body || el === document.documentElement) return "nothing";
  if (el === list) return "list";
  if (
    el instanceof HTMLInputElement ||
    el instanceof HTMLTextAreaElement ||
    el instanceof HTMLSelectElement ||
    (el instanceof HTMLElement && el.isContentEditable)
  ) {
    return "text";
  }
  return "control";
}

/**
 * Keeps keyboard focus on `list` while it is mounted: when the page opens or
 * the window comes back with focus nowhere, when focus is about to land
 * nowhere (a click on a spot that takes none, a menu or toast closing), and
 * after a click leaves it on a button. `focus` moves it there without the
 * keyboard focus ring. Returns the cleanup.
 */
export function holdListFocus(list: HTMLElement, focus: () => void): () => void {
  let pointerDown = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const check = (afterClick: boolean, enteringPage = false) => {
    clearTimeout(timer);
    // After the event's default actions and handlers: a click has done what
    // it does, a closing menu has let go of focus.
    timer = setTimeout(() => {
      if (!list.isConnected) return;
      const selection = document.getSelection();
      const state: FocusState = {
        holder: holderOf(document.activeElement, list),
        windowFocused: document.hasFocus(),
        popupOpen: !!document.querySelector('[aria-modal="true"], [role="menu"], [role="listbox"]'),
        pointerDown,
        selecting: !!selection && !selection.isCollapsed,
        afterClick,
        enteringPage,
      };
      if (reclaimsFocus(state)) focus();
    });
  };
  const onPointerDown = () => {
    pointerDown = true;
  };
  const onPointerUp = () => {
    pointerDown = false;
    check(true);
  };
  const onFocusOut = (e: FocusEvent) => {
    if (!e.relatedTarget) check(false);
  };
  // A window drag or another application can swallow the pointer's release.
  const onWindowFocus = () => {
    pointerDown = false;
    check(false);
  };
  const onWindowBlur = () => {
    pointerDown = false;
  };
  window.addEventListener("pointerdown", onPointerDown, true);
  window.addEventListener("pointerup", onPointerUp, true);
  window.addEventListener("pointercancel", onPointerUp, true);
  document.addEventListener("focusout", onFocusOut);
  window.addEventListener("focus", onWindowFocus);
  window.addEventListener("blur", onWindowBlur);
  // Navigation mounts this list in its click handler, after pointerup has
  // already fired. Explicitly hand off its focus rather than waiting for a
  // release this instance never saw; keyboard page switches need it too.
  check(false, true);
  return () => {
    clearTimeout(timer);
    window.removeEventListener("pointerdown", onPointerDown, true);
    window.removeEventListener("pointerup", onPointerUp, true);
    window.removeEventListener("pointercancel", onPointerUp, true);
    document.removeEventListener("focusout", onFocusOut);
    window.removeEventListener("focus", onWindowFocus);
    window.removeEventListener("blur", onWindowBlur);
  };
}
