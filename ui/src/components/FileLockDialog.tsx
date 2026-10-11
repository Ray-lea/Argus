// "Who is using this file?" — opened from the toolbar or by dropping files on the window.

import { createSignal, For, Match, onCleanup, Show, Switch } from "solid-js";
import { endProcessByPid } from "../lib/actions";
import { api } from "../lib/ipc";
import { presence } from "../lib/motion";
import { appInfo, revealProcess, snapshot } from "../lib/store";
import type { ActionError, Locker, LockReport } from "../lib/types";
import { EndIcon, FolderIcon, GoToIcon, LockIcon } from "./icons";
import ProcIcon from "./ProcIcon";

type State =
  | { kind: "idle" }
  | { kind: "searching" }
  | { kind: "done"; report: LockReport }
  | { kind: "error"; message: string };

const [open, setOpen] = createSignal(false);
const [paths, setPaths] = createSignal<string[]>([]);
const [state, setState] = createSignal<State>({ kind: "idle" });

const KIND_LABEL: Partial<Record<Locker["kind"], string>> = {
  service: "服务",
  explorer: "资源管理器",
  console: "控制台",
  critical: "系统关键",
};

async function search() {
  if (paths().length === 0) return;
  setState({ kind: "searching" });
  try {
    setState({ kind: "done", report: await api.findLockers(paths()) });
  } catch (e) {
    const err = e as ActionError;
    setState({
      kind: "error",
      message:
        err?.kind === "notFound"
          ? "找不到这个文件或文件夹。"
          : err?.kind === "os"
            ? err.message
            : `检查失败：${String(e)}`,
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
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") close();
  };
  window.addEventListener("keydown", onKey);
  onCleanup(() => window.removeEventListener("keydown", onKey));
  const procOf = (pid: number) => snapshot()?.processes.find((p) => p.pid === pid);

  return (
    <div
      class="scrim"
      classList={{ leaving: props.leaving }}
      onMouseDown={(e) => e.target === e.currentTarget && close()}
    >
      <div class="dialog wide" role="dialog" aria-modal="true" aria-labelledby="lock-title">
        <div class="dialog-body">
          <h2 id="lock-title" class="dialog-title">
            谁在占用文件
          </h2>
          <p class="dialog-text">
            删除或移动时提示“文件正在使用”？在这里找出占用它的进程。也可以直接把文件拖到 Argus 窗口上。
          </p>

          <div class="lock-paths">
            <Show
              when={paths().length > 0}
              fallback={
                <div class="drop-zone">
                  <LockIcon size={20} />
                  <span>把文件或文件夹拖到窗口上，或者</span>
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
                选择文件
              </button>
              <button type="button" class="btn" onClick={() => pick(true)}>
                <FolderIcon />
                选择文件夹
              </button>
              <Show when={paths().length > 0}>
                <button
                  type="button"
                  class="btn ghost"
                  disabled={state().kind === "searching"}
                  onClick={() => void search()}
                >
                  重新检查
                </button>
              </Show>
            </div>
          </div>

          <div class="lock-result" aria-live="polite">
            <Switch>
              <Match when={state().kind === "searching"}>
                <p class="dialog-text">正在检查…</p>
                <div class="progress lock-progress" />
              </Match>
              <Match when={state().kind === "error" && (state() as { message: string }).message}>
                {(msg) => <p class="setting-error">{msg()}</p>}
              </Match>
              <Match when={state().kind === "done" && (state() as { report: LockReport }).report}>
                {(report) => (
                  <div class="lock-report">
                    <Show
                      when={report().lockers.length > 0}
                      fallback={
                        <Show
                          when={report().filesChecked > 0}
                          fallback={
                            <p class="dialog-text">
                              Windows 不允许检查它：它正被受保护的系统进程使用，常见于系统 DLL。
                            </p>
                          }
                        >
                          <p class="dialog-text">
                            没有进程占用它。
                            <Show when={!appInfo()?.elevated}>
                              {" "}
                              以管理员身份运行 Argus 可以检查到更多进程。
                            </Show>
                          </p>
                        </Show>
                      }
                    >
                      <p class="dialog-text">
                        {report().lockers.length} 个进程正在使用，检查了 {report().filesChecked} 个文件。
                      </p>
                      <div class="locker-list">
                        <For each={report().lockers}>
                          {(l) => {
                            const proc = procOf(l.pid);
                            return (
                              <div class="locker">
                                <ProcIcon proc={{ icon: proc?.icon ?? null, path: proc?.path ?? null }} />
                                <div class="locker-name">
                                  <div class="label">{proc?.description ?? l.appName}</div>
                                  <div class="dim">
                                    {proc?.name ?? l.appName}　PID {l.pid}
                                    <Show when={l.service}>　服务 {l.service}</Show>
                                  </div>
                                </div>
                                <Show when={KIND_LABEL[l.kind]}>
                                  <span class="tag">{KIND_LABEL[l.kind]}</span>
                                </Show>
                                <button
                                  type="button"
                                  class="btn small ghost"
                                  title="在进程列表中查看"
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
                                  onClick={async () => {
                                    if (await endProcessByPid(l.pid, l.appName)) await search();
                                  }}
                                >
                                  <EndIcon size={14} />
                                  结束
                                </button>
                              </div>
                            );
                          }}
                        </For>
                      </div>
                    </Show>
                    <Show when={report().unchecked > 0 && report().filesChecked > 0}>
                      <p class="note">
                        另有 {report().unchecked} 个文件 Windows
                        不允许检查，通常是被受保护的系统进程加载的系统文件。
                      </p>
                    </Show>
                    <Show when={report().truncated}>
                      <p class="note">
                        文件夹里的文件很多，只检查了前 {report().filesChecked + report().unchecked} 个。
                      </p>
                    </Show>
                  </div>
                )}
              </Match>
            </Switch>
          </div>
        </div>
        <div class="dialog-footer">
          <span class="dialog-hint">以文件夹为当前目录的进程（例如打开着它的命令行）不会被检查到。</span>
          <button type="button" class="btn" onClick={close}>
            关闭
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
          <p>松开以查找占用这些文件的进程</p>
        </div>
      </div>
    </Show>
  );
}
