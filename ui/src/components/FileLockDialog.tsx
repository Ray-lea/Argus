// "Who is using this file?" — opened from the toolbar or by dropping files on the window.

import { createEffect, createSignal, For, Match, on, onCleanup, Show, Switch } from "solid-js";
import { endProcessByPid } from "../lib/actions";
import { t } from "../lib/i18n";
import { api } from "../lib/ipc";
import { presence } from "../lib/motion";
import { page, revealProcess, snapshot } from "../lib/store";
import type { ActionError, Locker, LockReport } from "../lib/types";
import { EndIcon, FolderIcon, GoToIcon, LockIcon } from "./icons";
import ProcIcon from "./ProcIcon";

type State =
  | { kind: "idle" }
  | { kind: "searching" }
  | { kind: "done"; report: LockReport }
  | { kind: "error"; message: () => string };

const [open, setOpen] = createSignal(false);
const [paths, setPaths] = createSignal<string[]>([]);
const [state, setState] = createSignal<State>({ kind: "idle" });
let searchVersion = 0;

/** Kinds worth a tag; windowed apps and the rest go without. */
const TAGGED = ["service", "explorer", "console", "critical"] as const;
type TaggedKind = (typeof TAGGED)[number];
const isTagged = (kind: Locker["kind"]): kind is TaggedKind => (TAGGED as readonly string[]).includes(kind);
const kindLabel = (kind: Locker["kind"]) => (isTagged(kind) ? t(`fileLock.kind.${kind}`) : undefined);

async function search() {
  if (paths().length === 0) return;
  const version = ++searchVersion;
  const selectedPaths = [...paths()];
  setState({ kind: "searching" });
  try {
    const report = await api.findLockers(selectedPaths);
    if (version === searchVersion) setState({ kind: "done", report });
  } catch (e) {
    if (version !== searchVersion) return;
    const err = e as ActionError;
    setState({
      kind: "error",
      message:
        err?.kind === "notFound"
          ? () => t("fileLock.error.notFound")
          : err?.kind === "os"
            ? () => err.message
            : () => t("fileLock.error.failed", { error: String(e) }),
    });
  }
}

/** Opens the dialog; with paths, checks them right away. */
export function openFileLock(newPaths?: string[]) {
  setOpen(true);
  if (newPaths && newPaths.length > 0) {
    setPaths(newPaths);
    void search();
  }
}

async function pick(folder: boolean) {
  const p = await api.pickPath(folder);
  if (p) openFileLock([p]);
}

const baseName = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;

export default function FileLockDialog() {
  const shown = presence(open, 170);
  return (
    <Show when={shown.value()}>
      <LockDialogBody leaving={shown.leaving()} />
    </Show>
  );
}

function LockDialogBody(props: { leaving: boolean }) {
  const close = () => setOpen(false);
  // Whatever leads to another page (a notice's "update service" after ending a
  // process here, say) leads away from this dialog too, as "show process" does:
  // left open, it would cover what that page points out.
  createEffect(on(page, close, { defer: true }));
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") close();
  };
  window.addEventListener("keydown", onKey);
  onCleanup(() => window.removeEventListener("keydown", onKey));
  const procOf = (locker: Locker) =>
    snapshot()?.processes.find((p) => p.pid === locker.pid && p.createTime === locker.createTime);

  return (
    <div
      class="scrim"
      classList={{ leaving: props.leaving }}
      onMouseDown={(e) => e.target === e.currentTarget && close()}
    >
      <div class="dialog wide lock-dialog" role="dialog" aria-modal="true" aria-labelledby="lock-title">
        <div class="dialog-body">
          <h2 id="lock-title" class="dialog-title">
            {t("fileLock.title")}
          </h2>
          <p class="dialog-text">{t("fileLock.intro")}</p>

          <div class="lock-paths">
            <Show
              when={paths().length > 0}
              fallback={
                <div class="drop-zone">
                  <LockIcon size={20} />
                  <span>{t("fileLock.dropZone")}</span>
                </div>
              }
            >
              <For each={paths()}>
                {(p) => (
                  <div class="path-chip" title={p}>
                    <span class="path-name">{baseName(p)}</span>
                    <span class="path-dir">{p}</span>
                  </div>
                )}
              </For>
            </Show>
            <div class="lock-pick">
              <button type="button" class="btn" onClick={() => pick(false)}>
                {t("fileLock.pickFile")}
              </button>
              <button type="button" class="btn" onClick={() => pick(true)}>
                <FolderIcon />
                {t("fileLock.pickFolder")}
              </button>
              <Show when={paths().length > 0}>
                <button
                  type="button"
                  class="btn ghost"
                  disabled={state().kind === "searching"}
                  onClick={() => void search()}
                >
                  {t("fileLock.recheck")}
                </button>
              </Show>
            </div>
          </div>

          <div class="lock-result" aria-live="polite">
            <Switch>
              <Match when={state().kind === "searching"}>
                <p class="dialog-text">{t("fileLock.checking")}</p>
                <div class="progress lock-progress" />
              </Match>
              <Match when={state().kind === "error" && (state() as { message: () => string }).message}>
                {(msg) => <p class="setting-error">{msg()()}</p>}
              </Match>
              <Match when={state().kind === "done" && (state() as { report: LockReport }).report}>
                {(report) => (
                  <div class="lock-report">
                    <Show
                      when={report().lockers.length > 0}
                      fallback={<p class="dialog-text">{t("fileLock.none")}</p>}
                    >
                      <p class="dialog-text">{t("fileLock.found", { count: report().lockers.length })}</p>
                      <div class="locker-list">
                        <For each={report().lockers}>
                          {(l) => {
                            const proc = () => procOf(l);
                            return (
                              <div class="locker">
                                <ProcIcon proc={{ icon: proc()?.icon ?? null, path: proc()?.path ?? null }} />
                                <div class="locker-name">
                                  <div class="label">{proc()?.description ?? l.appName}</div>
                                  <div class="dim">
                                    {proc()?.name ?? l.appName}　PID {l.pid}
                                    <Show when={l.service}>
                                      {" "}
                                      {t("fileLock.service", { name: l.service ?? "" })}
                                    </Show>
                                  </div>
                                  <Show when={l.handlePath}>
                                    {(path) => (
                                      <div class="dim label" title={path()}>
                                        {path()}
                                      </div>
                                    )}
                                  </Show>
                                </div>
                                <Show when={kindLabel(l.kind)}>
                                  {(label) => <span class="tag">{label()}</span>}
                                </Show>
                                <Show when={l.handleCount > 0}>
                                  <span class="tag">
                                    {t("fileLock.handleCount", { count: l.handleCount })}
                                  </span>
                                </Show>
                                <button
                                  type="button"
                                  class="btn small ghost"
                                  title={t("fileLock.showProcess")}
                                  disabled={!proc()}
                                  onClick={() => {
                                    close();
                                    revealProcess(l.pid);
                                  }}
                                >
                                  <GoToIcon size={14} />
                                </button>
                                <button
                                  type="button"
                                  class="btn small danger"
                                  disabled={l.createTime <= 0}
                                  onClick={async () => {
                                    if (await endProcessByPid(l.pid, l.appName, l.createTime)) await search();
                                  }}
                                >
                                  <EndIcon size={14} />
                                  {t("fileLock.end")}
                                </button>
                              </div>
                            );
                          }}
                        </For>
                      </div>
                    </Show>
                    <Show when={report().handles}>
                      {(handles) => (
                        <>
                          <p class="note">
                            {t("fileLock.checked", {
                              count: report().filesChecked,
                              handles: handles().available
                                ? t(
                                    handles().viaService
                                      ? "fileLock.handles.service"
                                      : "fileLock.handles.local",
                                    {
                                      count: handles().checked,
                                    },
                                  )
                                : t("fileLock.handles.unfinished"),
                            })}
                          </p>
                          <Show
                            when={
                              handles().deniedProcesses > 0 ||
                              handles().skipped > 0 ||
                              handles().timeouts > 0 ||
                              handles().truncated
                            }
                          >
                            <p class="note">
                              {t("fileLock.incomplete", {
                                denied: handles().deniedProcesses,
                                skipped: handles().skipped,
                                timeouts: handles().timeouts,
                                limit: handles().truncated ? t("fileLock.limitReached") : "",
                              })}
                            </p>
                          </Show>
                          <Show when={!handles().viaService && !handles().elevated}>
                            <p class="note">{t("fileLock.serviceHint")}</p>
                          </Show>
                          <Show when={handles().error}>{(error) => <p class="note">{error()}</p>}</Show>
                        </>
                      )}
                    </Show>
                    <Show when={report().restartError}>{(error) => <p class="note">{error()}</p>}</Show>
                    <Show when={report().unchecked > 0}>
                      <p class="note">{t("fileLock.unchecked", { count: report().unchecked })}</p>
                    </Show>
                    <Show when={report().truncated}>
                      <p class="note">
                        {t("fileLock.truncated", { count: report().filesChecked + report().unchecked })}
                      </p>
                    </Show>
                  </div>
                )}
              </Match>
            </Switch>
          </div>
        </div>
        <div class="dialog-footer">
          <span class="dialog-hint">{t("fileLock.footerHint")}</span>
          <button type="button" class="btn" onClick={close}>
            {t("common.close")}
          </button>
        </div>
      </div>
    </div>
  );
}

/** Full-window hint while files are dragged over Argus. */
export function DropOverlay(props: { visible: boolean }) {
  const shown = presence(() => props.visible, 120);
  return (
    <Show when={shown.value()}>
      <div class="drop-overlay" classList={{ leaving: shown.leaving() }} aria-hidden="true">
        <div>
          <LockIcon size={28} />
          <p>{t("fileLock.drop")}</p>
        </div>
      </div>
    </Show>
  );
}
