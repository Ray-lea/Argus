import { createEffect, Show } from "solid-js";
import { t } from "../lib/i18n";
import { DismissIcon, SearchIcon } from "./icons";

/** Room besides the text: the icon on the left, padding on the right, borders. */
const CHROME_PX = 34 + 12 + 2;
let measure: CanvasRenderingContext2D | null = null;

/** Width of `text` in `input`'s font, in CSS pixels. */
function textWidth(input: HTMLInputElement, text: string) {
  measure ??= document.createElement("canvas").getContext("2d");
  if (!measure) return 0;
  const style = getComputedStyle(input);
  measure.font = `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
  return measure.measureText(text).width;
}

export default function SearchBox(props: {
  value: string;
  onInput: (value: string) => void;
  placeholder: string;
  ref?: (el: HTMLInputElement) => void;
  /** After Esc has cleared the box and left it. */
  onEscape?: () => void;
  /** ↓ moves on to the results. */
  onNavigate?: () => void;
}) {
  let box!: HTMLLabelElement;
  let input!: HTMLInputElement;
  // Never narrower than the placeholder: a toolbar short of room sheds labels
  // elsewhere first (see fitToolbar) instead of cutting the hint off.
  createEffect(() => {
    const width = Math.ceil(textWidth(input, props.placeholder)) + CHROME_PX;
    box.style.setProperty("--fit", `${width}px`);
  });
  return (
    <label ref={box} class="search" classList={{ filled: !!props.value }}>
      <SearchIcon />
      <input
        ref={(el) => {
          input = el;
          props.ref?.(el);
        }}
        type="search"
        placeholder={props.placeholder}
        aria-label={props.placeholder}
        spellcheck={false}
        value={props.value}
        onInput={(e) => props.onInput(e.currentTarget.value)}
        onKeyDown={(e) => {
          // Keys inside an input method's composition (Esc cancels it, ↓
          // picks a candidate) are the input method's.
          if (e.isComposing) return;
          if (e.key === "Escape") {
            props.onInput("");
            e.currentTarget.blur();
            props.onEscape?.();
          } else if (e.key === "ArrowDown" && props.onNavigate) {
            e.preventDefault();
            props.onNavigate();
          }
        }}
      />
      <Show when={props.value}>
        <button
          type="button"
          class="btn ghost small icon clear"
          title={t("shell.search.clear")}
          onClick={() => props.onInput("")}
        >
          <DismissIcon size={14} />
        </button>
      </Show>
    </label>
  );
}
