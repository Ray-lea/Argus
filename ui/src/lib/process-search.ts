// The processes page's search box as the rest of the page reaches it: Ctrl+F
// and /, letters typed in the list (an input method's included), and back.

import { createSignal } from "solid-js";
import { query, setQuery } from "./store";

let input: HTMLInputElement | undefined;
/** What had focus before the search took it: Esc goes back there. */
let returnFocus: HTMLElement | null = null;

/**
 * What an input method is composing in the list for the search: shown in the
 * box after the query, searched for only once committed.
 */
export const [preedit, setPreedit] = createSignal("");

export function setSearchInput(el: HTMLInputElement) {
  input = el;
}

function rememberFocus() {
  const active = document.activeElement;
  returnFocus = active instanceof HTMLElement && active !== document.body && active !== input ? active : null;
}

/** After Esc has cleared the box: back to what had focus before it. */
export function leaveSearch() {
  if (returnFocus?.isConnected) returnFocus.focus();
  returnFocus = null;
}

/** Leaving the box some other way (↓ to the list): Esc there no longer goes back. */
export function forgetReturn() {
  returnFocus = null;
}

export function focusSearch() {
  rememberFocus();
  input?.focus();
  input?.select();
}

/**
 * Starts the process search with `text` typed outside any text field. It goes
 * after what the box already holds, so a stray key costs no filter, and the
 * typing carries on in the box. False when the box is not there.
 */
export function typeIntoSearch(text: string): boolean {
  const box = input;
  if (!box?.isConnected) return false;
  rememberFocus();
  const next = query() + text;
  setQuery(next);
  box.focus();
  box.setSelectionRange(next.length, next.length);
  return true;
}

let measure: CanvasRenderingContext2D | null = null;

/**
 * The search box and the end of its text, in client coordinates: an input
 * method composing in the list opens its window there.
 */
function searchBounds(): { control: DOMRect; caret: DOMRect } | null {
  if (!input?.isConnected) return null;
  const box = input.getBoundingClientRect();
  const style = getComputedStyle(input);
  measure ??= document.createElement("canvas").getContext("2d");
  let width = 0;
  if (measure) {
    measure.font = `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
    width = measure.measureText(input.value).width;
  }
  const start = box.left + Number.parseFloat(style.paddingLeft);
  const end = box.right - Number.parseFloat(style.paddingRight);
  const x = Math.min(start + width, end);
  return { control: box, caret: new DOMRect(x, box.top + 4, 1, Math.max(1, box.height - 8)) };
}

// The parts of the EditContext API used here; TypeScript's DOM types lack it.
interface CompositionTarget extends EventTarget {
  readonly text: string;
  updateText(start: number, end: number, text: string): void;
  updateSelection(start: number, end: number): void;
  updateControlBounds(bounds: DOMRect): void;
  updateSelectionBounds(bounds: DOMRect): void;
  updateCharacterBounds(start: number, bounds: DOMRect[]): void;
}
type Editable = HTMLElement & { editContext: CompositionTarget | null };

/**
 * Lets an input method type into the search from `el` (the process list).
 * Input methods only compose in an editable element, so without this the
 * first key in the list would arrive as a bare letter. An EditContext makes
 * the list one without changing what it is: plain keys still reach it (and
 * App.tsx), while a composition shows in the search box as it goes, its
 * window opening under the box, and moves there once committed. Returns
 * the cleanup; does nothing where EditContext is missing.
 */
export function composeIntoSearch(el: HTMLElement): () => void {
  const Context = (window as { EditContext?: new () => CompositionTarget }).EditContext;
  if (!Context) return () => {};
  const context = new Context();
  const editable = el as Editable;
  editable.editContext = context;
  // No on-screen keyboard just for focusing the list on a touch screen.
  el.setAttribute("virtualkeyboardpolicy", "manual");
  let composing = false;
  const take = () => {
    const text = context.text;
    context.updateText(0, text.length, "");
    context.updateSelection(0, 0);
    return text;
  };
  const place = () => {
    const bounds = searchBounds();
    if (bounds) {
      context.updateControlBounds(bounds.control);
      context.updateSelectionBounds(bounds.caret);
    }
    return bounds;
  };
  const onStart = () => {
    composing = true;
    place();
  };
  const onUpdate = () => {
    // Text outside a composition (punctuation the method passes on) starts
    // no search: letters and digits arrive as keys, which App.tsx handles.
    if (composing) setPreedit(context.text);
    else take();
  };
  const onBounds = (e: Event) => {
    const { rangeStart, rangeEnd } = e as Event & { rangeStart: number; rangeEnd: number };
    const bounds = place();
    if (bounds) {
      context.updateCharacterBounds(
        rangeStart,
        Array.from({ length: Math.max(0, rangeEnd - rangeStart) }, () => bounds.caret),
      );
    }
  };
  const onEnd = () => {
    composing = false;
    const text = take();
    setPreedit("");
    // Once the method is done with the list, and only if the list still has
    // focus: a click elsewhere also ends a composition.
    if (/[\p{L}\p{N}]/u.test(text)) {
      setTimeout(() => {
        if (document.activeElement === el) typeIntoSearch(text);
      });
    }
  };
  context.addEventListener("compositionstart", onStart);
  context.addEventListener("textupdate", onUpdate);
  context.addEventListener("characterboundsupdate", onBounds);
  context.addEventListener("compositionend", onEnd);
  return () => {
    context.removeEventListener("compositionstart", onStart);
    context.removeEventListener("textupdate", onUpdate);
    context.removeEventListener("characterboundsupdate", onBounds);
    context.removeEventListener("compositionend", onEnd);
    if (editable.editContext === context) editable.editContext = null;
    el.removeAttribute("virtualkeyboardpolicy");
    setPreedit("");
  };
}
