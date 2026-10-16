// A collapsible section of the performance page, like a Windows 11 expander:
// the header names the section and keeps a live one-line summary, so a closed
// section still answers the common question; the content exists only while
// open, so a closed section costs no updates. Which sections are open is kept
// across restarts; all start closed.
import { type JSX, Show } from "solid-js";
import { EASE_OUT, play } from "../lib/motion";
import { persisted } from "../lib/prefs";
import { ChevronRight } from "./icons";

const [openIds, setOpenIds] = persisted<Record<string, boolean>>("perfSections", {});

export default function Expander(props: {
  /** Stable key for the open state, e.g. "cpu.cores". */
  id: string;
  title: string;
  /** Beside the title, e.g. the sensor chip's name. */
  note?: string | null;
  summary?: string | null;
  /** Something to notice even while closed (an active limit). */
  alert?: boolean;
  children: JSX.Element;
}) {
  const open = () => openIds()[props.id] ?? false;
  let body: HTMLDivElement | undefined;
  const toggle = () => {
    const next = !open();
    setOpenIds({ ...openIds(), [props.id]: next });
    if (next)
      queueMicrotask(() =>
        play(
          body,
          [
            { opacity: 0, transform: "translateY(-4px)" },
            { opacity: 1, transform: "none" },
          ],
          { duration: 180, easing: EASE_OUT },
        ),
      );
  };
  return (
    <section class="expander" classList={{ open: open(), alert: props.alert }}>
      <button type="button" class="expander-head" aria-expanded={open()} onClick={toggle}>
        <ChevronRight size={12} class="expander-chevron" />
        <span class="expander-title">
          {props.title}
          <Show when={props.note}>
            <small>{props.note}</small>
          </Show>
        </span>
        {/* The title shows a summary cut short by a narrow window in full. */}
        <span class="expander-summary" title={props.summary ?? undefined}>
          {props.summary}
        </span>
      </button>
      <Show when={open()}>
        <div ref={body} class="expander-body">
          {props.children}
        </div>
      </Show>
    </section>
  );
}
