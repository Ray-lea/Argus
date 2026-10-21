// Notes and pins on the services, startup and connections tables: the note
// after a row's name, the pin at the end of its first cell, their menu items
// and the dialog that edits a note.

import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { plainEscape } from "../lib/escape";
import { t } from "../lib/i18n";
import type { RowMarks } from "../lib/marks";
import { presence } from "../lib/motion";
import { NOTE_MAX } from "../lib/row-marks";
import type { MenuEntry } from "../lib/ui";
import { NoteIcon, PinIcon, TrashIcon, UnpinIcon } from "./icons";

/** Right after the row's name. */
export function NoteBadge(props: { note: string | undefined }) {
  return (
    <Show when={props.note}>
      {(note) => (
        <span class="row-note" title={note()}>
          <NoteIcon size={12} />
          <span class="row-note-text">{note()}</span>
        </span>
      )}
    </Show>
  );
}

/** At the end of the row's first cell. */
export function PinMark(props: { pinned: boolean }) {
  return (
    <Show when={props.pinned}>
      <span class="row-pin" role="img" aria-label={t("common.marks.pinned")} title={t("common.marks.pinned")}>
        <PinIcon size={12} />
      </span>
    </Show>
  );
}

/**
 * Pin and note items for a row's menu. `key` is what they are kept under,
 * null when the row has nothing stable to keep them under; `name` says what
 * the note is for in its dialog.
 */
export function markMenu(marks: RowMarks, key: string | null, name: string): MenuEntry[] {
  const pinned = key !== null && marks.pinned(key);
  const note = key === null ? undefined : marks.note(key);
  // The table, focused by the right click: the dialog gives focus back to it.
  const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const items: MenuEntry[] = [
    {
      label: t(pinned ? "common.marks.unpin" : "common.marks.pin"),
      icon: () => (pinned ? <UnpinIcon /> : <PinIcon />),
      disabled: key === null,
      run: () => key !== null && marks.setPinned(key, !pinned),
    },
    {
      label: t(note ? "common.marks.editNote" : "common.marks.addNote"),
      icon: () => <NoteIcon />,
      disabled: key === null,
      run: () =>
        key !== null &&
        setRequest({ name, note: note ?? "", save: (text) => marks.setNote(key, text), opener }),
    },
  ];
  if (key !== null && note) {
    items.push({
      label: t("common.marks.deleteNote"),
      icon: () => <TrashIcon />,
      run: () => marks.setNote(key, ""),
    });
  }
  return items;
}

interface NoteRequest {
  name: string;
  note: string;
  save: (note: string) => void;
  opener: HTMLElement | null;
}

const [request, setRequest] = createSignal<NoteRequest | null>(null);

export default function NoteDialog() {
  const shown = presence(request, 170);
  return (
    <Show when={shown.value()} keyed>
      {(req) => <NoteForm req={req} leaving={shown.leaving()} />}
    </Show>
  );
}

function NoteForm(props: { req: NoteRequest; leaving: boolean }) {
  const [text, setText] = createSignal(props.req.note);
  let form!: HTMLFormElement, input!: HTMLInputElement;
  const close = () => {
    if (request() === props.req) setRequest(null);
  };
  const save = (event: SubmitEvent) => {
    event.preventDefault();
    if (request() !== props.req) return;
    props.req.save(text());
    close();
  };
  const onKey = (event: KeyboardEvent) => {
    if (plainEscape(event)) {
      event.preventDefault();
      event.stopImmediatePropagation();
      close();
    } else if (event.key === "Tab") {
      const targets = [...form.querySelectorAll<HTMLElement>("button, input")];
      const index = targets.indexOf(document.activeElement as HTMLElement);
      if (index < 0 || (event.shiftKey ? index === 0 : index === targets.length - 1)) {
        event.preventDefault();
        targets[event.shiftKey ? targets.length - 1 : 0]?.focus();
      }
    }
  };
  onMount(() => {
    input.focus();
    input.select();
    window.addEventListener("keydown", onKey, true);
    onCleanup(() => {
      window.removeEventListener("keydown", onKey, true);
      if (props.req.opener?.isConnected) props.req.opener.focus();
    });
  });
  return (
    <div
      class="scrim"
      classList={{ leaving: props.leaving }}
      onMouseDown={(event) => event.target === event.currentTarget && close()}
    >
      <form
        ref={form}
        class="dialog note-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="note-title"
        aria-describedby="note-target"
        onSubmit={save}
      >
        <div class="dialog-body">
          <h2 id="note-title" class="dialog-title">
            {t(props.req.note ? "common.marks.editNote" : "common.marks.addNote")}
          </h2>
          <p id="note-target" class="note-target" title={props.req.name}>
            {props.req.name}
          </p>
          <input
            ref={input}
            class="input"
            type="text"
            value={text()}
            maxLength={NOTE_MAX}
            spellcheck={false}
            autocomplete="off"
            placeholder={t("common.marks.placeholder")}
            aria-label={t("common.marks.note")}
            onInput={(event) => setText(event.currentTarget.value)}
          />
        </div>
        <div class="dialog-footer">
          <button type="submit" class="btn accent">
            {t("common.marks.save")}
          </button>
          <button type="button" class="btn" onClick={close}>
            {t("common.cancel")}
          </button>
        </div>
      </form>
    </div>
  );
}
