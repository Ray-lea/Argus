import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { plainEscape } from "../lib/escape";
import { type MessageKey, t } from "../lib/i18n";
import { api } from "../lib/ipc";
import { presence } from "../lib/motion";
import { quotedTaskPath, validTaskCommand } from "../lib/task-command";
import type { TaskError } from "../lib/types";
import { toast } from "../lib/ui";
import { DismissIcon, FolderIcon, RunTaskIcon, ShieldIcon } from "./icons";

const [request, setRequest] = createSignal<{ command: string; opener: HTMLElement | null } | null>(null);
const ERRORS: Record<TaskError["kind"], MessageKey> = {
  invalid: "tasks.error.invalid",
  cancelled: "tasks.error.cancelled",
  notFound: "tasks.error.notFound",
  accessDenied: "tasks.error.accessDenied",
  standardUnavailable: "tasks.error.standardUnavailable",
  elevationRequired: "tasks.error.elevationRequired",
  noAssociation: "tasks.error.noAssociation",
  busy: "tasks.error.busy",
  os: "tasks.error.os",
};

export function openNewTask(command = "", opener?: HTMLElement) {
  if (request()) {
    document.getElementById("new-task-command")?.focus();
    return;
  }
  setRequest({ command, opener: opener ?? (document.activeElement as HTMLElement | null) });
}

export default function NewTaskDialog() {
  const shown = presence(() => request() !== null, 170);
  return (
    <Portal>
      <Show when={shown.value()}>
        <TaskDialog leaving={shown.leaving()} />
      </Show>
    </Portal>
  );
}

function TaskDialog(props: { leaving: boolean }) {
  const initial = request();
  const [command, setCommand] = createSignal(initial?.command ?? "");
  const [elevated, setElevated] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [picking, setPicking] = createSignal(false);
  const [error, setError] = createSignal<TaskError | null>(null);
  let dialog!: HTMLFormElement, input!: HTMLInputElement;
  const pending = () => busy() || picking();
  const close = () => {
    if (!pending()) setRequest(null);
  };
  const fail = (value: unknown) => {
    const err = value as Partial<TaskError> | null;
    setError({
      kind: err?.kind && Object.hasOwn(ERRORS, err.kind) ? err.kind : "os",
      code: typeof err?.code === "number" ? err.code : 0,
    });
  };
  const run = async (event: SubmitEvent) => {
    event.preventDefault();
    if (pending()) return;
    if (!validTaskCommand(command())) {
      setError({ kind: "invalid", code: 0 });
      input.focus();
      return;
    }
    setError(null);
    setBusy(true);
    let success = false;
    try {
      await api.runTask(command().trim(), elevated());
      success = true;
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
    if (success) {
      toast("success", t("tasks.started"));
      close();
    } else {
      queueMicrotask(() => input.focus());
    }
  };
  const browse = async () => {
    if (pending()) return;
    setPicking(true);
    try {
      const path = await api.pickTaskFile();
      if (path) {
        setCommand(quotedTaskPath(path));
        setError(null);
      }
    } catch (err) {
      fail(err);
    } finally {
      setPicking(false);
      input.focus();
    }
  };
  const onKey = (event: KeyboardEvent) => {
    if (plainEscape(event)) {
      event.preventDefault();
      event.stopImmediatePropagation();
      close();
    } else if (event.key === "Tab") {
      const targets = [
        ...dialog.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled)"),
      ].filter((el) => el.getClientRects().length > 0);
      const index = targets.indexOf(document.activeElement as HTMLElement);
      if (index < 0 || (event.shiftKey ? index === 0 : index === targets.length - 1)) {
        event.preventDefault();
        targets[event.shiftKey ? targets.length - 1 : 0]?.focus();
      }
    }
  };
  onMount(() => {
    input.focus();
    if (command()) input.select();
    window.addEventListener("keydown", onKey, true);
    onCleanup(() => {
      window.removeEventListener("keydown", onKey, true);
      if (initial?.opener?.isConnected) initial.opener.focus();
    });
  });
  return (
    <div
      class="scrim"
      classList={{ leaving: props.leaving }}
      onMouseDown={(event) => event.target === event.currentTarget && close()}
    >
      <form
        ref={dialog}
        class="dialog new-task-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="new-task-title"
        aria-describedby="new-task-description"
        onSubmit={run}
      >
        <div class="new-task-header">
          <span class="new-task-emblem">
            <RunTaskIcon size={24} />
          </span>
          <div>
            <h2 id="new-task-title" class="dialog-title">
              {t("tasks.title")}
            </h2>
            <p id="new-task-description">{t("tasks.description")}</p>
          </div>
          <button
            type="button"
            class="btn icon"
            aria-label={t("common.close")}
            disabled={pending()}
            onClick={close}
          >
            <DismissIcon />
          </button>
        </div>
        <div class="new-task-body">
          <label class="new-task-label" for="new-task-command">
            {t("tasks.command")}
          </label>
          <div class="new-task-input">
            <input
              ref={input}
              id="new-task-command"
              type="text"
              value={command()}
              maxLength={8192}
              spellcheck={false}
              autocomplete="off"
              placeholder={t("tasks.placeholder")}
              disabled={pending()}
              aria-invalid={!!error() && error()?.kind !== "cancelled"}
              aria-describedby="new-task-feedback"
              onInput={(event) => {
                setCommand(event.currentTarget.value);
                setError(null);
              }}
            />
            <button type="button" class="btn" disabled={pending()} onClick={browse}>
              <FolderIcon />
              {t("tasks.browse")}
            </button>
          </div>
          <label class="new-task-privilege">
            <input
              type="checkbox"
              checked={elevated()}
              disabled={pending()}
              onChange={(event) => {
                setElevated(event.currentTarget.checked);
                setError(null);
              }}
            />
            <span>
              <span class="new-task-privilege-title">
                {t("tasks.administrator")}
                <ShieldIcon size={14} />
              </span>
              <span class="new-task-hint">
                {elevated() ? t("tasks.administratorHint") : t("tasks.standardHint")}
              </span>
            </span>
          </label>
          <div
            id="new-task-feedback"
            class="new-task-feedback"
            classList={{ error: !!error() && error()?.kind !== "cancelled" }}
            aria-live="polite"
            aria-atomic="true"
          >
            <Show when={error()} fallback={busy() ? t("tasks.waiting") : ""}>
              {(err) => (
                <>
                  <span>{t(ERRORS[err().kind])}</span>
                  <Show when={err().code && err().kind !== "cancelled"}>
                    <span class="new-task-code">
                      {t("tasks.code", {
                        code: `0x${err().code.toString(16).toUpperCase().padStart(8, "0")}`,
                      })}
                    </span>
                  </Show>
                </>
              )}
            </Show>
          </div>
        </div>
        <div class="dialog-footer">
          <button type="button" class="btn" disabled={pending()} onClick={close}>
            {t("common.cancel")}
          </button>
          <button type="submit" class="btn accent" disabled={pending() || !validTaskCommand(command())}>
            {busy() ? t("tasks.running") : t("tasks.run")}
          </button>
        </div>
      </form>
    </div>
  );
}
