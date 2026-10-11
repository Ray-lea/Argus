import { Show } from "solid-js";
import { DismissIcon, SearchIcon } from "./icons";

export default function SearchBox(props: {
  value: string;
  onInput: (value: string) => void;
  placeholder: string;
  ref?: (el: HTMLInputElement) => void;
}) {
  return (
    <label class="search">
      <SearchIcon />
      <input
        ref={props.ref}
        type="search"
        placeholder={props.placeholder}
        aria-label={props.placeholder}
        spellcheck={false}
        value={props.value}
        onInput={(e) => props.onInput(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            props.onInput("");
            e.currentTarget.blur();
          }
        }}
      />
      <Show when={props.value}>
        <button
          type="button"
          class="btn ghost small icon clear"
          title="清除搜索"
          onClick={() => props.onInput("")}
        >
          <DismissIcon size={14} />
        </button>
      </Show>
    </label>
  );
}
