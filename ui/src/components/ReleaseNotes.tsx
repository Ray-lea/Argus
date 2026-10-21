// Shared release reader for the settings card and update dialog.
import { createMemo, createSignal, For, Show } from "solid-js";
import { locale, localized, t } from "../lib/i18n";
import { EASE_OUT, play } from "../lib/motion";
import {
  type Block,
  between,
  changeCount,
  compareVersions,
  parseNotes,
  type Release,
  readable,
  type Span,
} from "../lib/release-notes";
import { ArrowRightIcon, ChevronRight } from "./icons";

const dayFmt = localized(
  (l) => new Intl.DateTimeFormat(l, { month: "long", day: "numeric", timeZone: "UTC" }),
);
const fullFmt = localized(
  (l) => new Intl.DateTimeFormat(l, { year: "numeric", month: "long", day: "numeric", timeZone: "UTC" }),
);

/** A `YYYY-MM-DD` changelog date; the year only when it is not this one. */
export function formatDate(date: string): string {
  const ms = Date.parse(`${date}T00:00:00Z`);
  if (Number.isNaN(ms)) return date;
  return (new Date(ms).getUTCFullYear() === new Date().getFullYear() ? dayFmt() : fullFmt()).format(ms);
}

export function UpdateVersions(props: { from: string | null; to: string; installed?: boolean }) {
  return (
    <Show when={props.to && props.from}>
      {(version) => (
        <div class="update-versions">
          <div class="update-version">
            <span class="update-version-label">
              {!props.installed ? t("updates.dialog.current") : t("updates.dialog.previous")}
            </span>
            <span class="update-version-number">{version()}</span>
          </div>
          <ArrowRightIcon class="update-arrow" />
          <div class="update-version next">
            <span class="update-version-label">
              {!props.installed ? t("updates.dialog.next") : t("updates.dialog.current")}
            </span>
            <span class="update-version-number">{props.to}</span>
          </div>
        </div>
      )}
    </Show>
  );
}

export default function ReleaseNotes(props: {
  releases?: Release[] | null;
  from: string | null;
  to: string;
  notes?: string | null;
  loading?: boolean;
  /** Every release open, not just the newest (the dialog, which has the room). */
  openAll?: boolean;
}) {
  // Progress events can carry new objects for unchanged releases. Keep each
  // entry mounted so reading position and expanded sections survive them.
  const entries = createMemo<ReturnType<typeof readable>[]>(
    (previous) =>
      between(props.releases ?? [], props.from, props.to)
        .map((entry) => readable(entry, locale()))
        .map(
          (entry) =>
            previous.find(
              (old) => old.version === entry.version && old.date === entry.date && old.notes === entry.notes,
            ) ?? entry,
        ),
    [],
  );
  const earlier = () =>
    !!props.releases?.length &&
    !!props.from &&
    props.releases.every((r) => compareVersions(r.version, props.from ?? "") > 0);
  const fallback = createMemo(() => parseNotes(props.notes ?? ""));
  return (
    // biome-ignore lint/a11y/noNoninteractiveTabindex: The named scroll region supports keyboard scrolling.
    <div class="update-notes" role="region" aria-label={t("settings.update.viewNotes")} tabIndex={0}>
      <Show
        when={entries().length > 0}
        fallback={
          <Show when={!props.loading}>
            <Show
              when={fallback().length > 0}
              fallback={<p class="update-quiet">{t("updates.dialog.empty")}</p>}
            >
              <div class="update-entry-body update-notes-fallback">
                <For each={fallback()}>{(block) => <BlockView block={block} />}</For>
              </div>
            </Show>
          </Show>
        }
      >
        <For each={entries()}>
          {(item, index) => <Entry entry={item} open={props.openAll || index() === 0} />}
        </For>
        <Show when={earlier()}>
          <p class="update-quiet">{t("updates.dialog.earlier")}</p>
        </Show>
        <Show when={entries().some((entry) => entry.untranslated)}>
          <p class="update-quiet">{t("updates.dialog.language")}</p>
        </Show>
      </Show>
    </div>
  );
}

/** One release: its version, date and size of change; starts open or closed as asked. */
function Entry(props: { entry: Release; open: boolean }) {
  const blocks = createMemo(() => parseNotes(props.entry.notes));
  const [open, setOpen] = createSignal(props.open);
  let body: HTMLDivElement | undefined;
  const toggle = () => {
    const next = !open();
    setOpen(next);
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
    <section class="update-entry" classList={{ open: open() }}>
      <button type="button" class="update-entry-head" aria-expanded={open()} onClick={toggle}>
        <ChevronRight size={12} class="update-entry-chevron" />
        <span class="update-entry-version">{props.entry.version}</span>
        <span class="update-entry-date">{formatDate(props.entry.date)}</span>
        <Show when={changeCount(blocks())}>
          {(count) => (
            <span class="update-entry-count">{t("updates.dialog.changes", { count: count() })}</span>
          )}
        </Show>
      </button>
      <Show when={open()}>
        <div ref={body} class="update-entry-body">
          <For each={blocks()}>{(block) => <BlockView block={block} />}</For>
        </div>
      </Show>
    </section>
  );
}

function BlockView(props: { block: Block }) {
  const block = props.block;
  if (block.kind === "text")
    return (
      <p class="update-text">
        <Spans spans={block.spans} />
      </p>
    );
  // Known headings in the reader's language; others as written.
  const tag = () =>
    block.title && (block.category === "other" ? block.title : t(`updates.category.${block.category}`));
  return (
    <div class="update-group" data-category={block.category}>
      <Show when={tag()}>
        <span class="update-tag">{tag()}</span>
      </Show>
      <ul class="update-items">
        <For each={block.items}>
          {(item) => (
            <li>
              <Spans spans={item} />
            </li>
          )}
        </For>
      </ul>
    </div>
  );
}

function Spans(props: { spans: Span[] }) {
  return (
    <For each={props.spans}>
      {(s) => (s.code ? <code>{s.text}</code> : s.strong ? <strong>{s.text}</strong> : s.text)}
    </For>
  );
}
