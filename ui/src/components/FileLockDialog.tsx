// "Who is using this file?" — opened from the toolbar or by dropping files on the window.

import { createSignal, For, Match, onCleanup, Show, Switch } from "solid-js";
import { endProcessByPid } from "../lib/actions";
import { api } from "../lib/ipc";
import { presence } from "../lib/motion";
import { revealProcess, snapshot } from "../lib/store";
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
let searchVersion = 0;

const KIND_LABEL: Partial<Record<Locker["kind"], string>> = {
  service: "服务",
  explorer: "资源管理器",
  console: "控制台",
  critical: "系统关键",
};

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
          ? "找不到该文件或文件夹。"
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
            谁在占用文件
          </h2>
          <p class="dialog-text">
            删除或移动文件时提示“文件正在使用”，可在此查找占用该文件的进程，也可将文件直接拖放到 Argus 窗口。
          </p>

          <div class="lock-paths">
            <Show
              when={paths().length > 0}
              fallback={
                <div class="drop-zone">
                  <LockIcon size={20} />
                  <span>将文件或文件夹拖放到窗口，或</span>
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
                      fallback={<p class="dialog-text">本次查询未发现可访问的占用进程。</p>}
                    >
                      <p class="dialog-text">
                        找到 {report().lockers.length} 个进程正在使用这些文件或文件夹。
                      </p>
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
                                    <Show when={l.service}>　服务 {l.service}</Show>
                                  </div>
                                  <Show when={l.handlePath}>
                                    {(path) => (
                                      <div class="dim label" title={path()}>
                                        {path()}
                                      </div>
                                    )}
                                  </Show>
                                </div>
                                <Show when={KIND_LABEL[l.kind]}>
                                  <span class="tag">{KIND_LABEL[l.kind]}</span>
                                </Show>
                                <Show when={l.handleCount > 0}>
                                  <span class="tag">{l.handleCount} 个句柄</span>
                                </Show>
                                <button
                                  type="button"
                                  class="btn small ghost"
                                  title="在进程列表中查看"
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
                                  结束
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
                            Restart Manager 检查了 {report().filesChecked} 个文件。
                            {handles().available
                              ? `${handles().viaService ? "特权服务" : "本地"}已检查 ${handles().checked} 个文件句柄，包括目录句柄。`
                              : "句柄查询未完成。"}
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
                              结果不完整：{handles().deniedProcesses} 个进程无法访问，{handles().skipped}{" "}
                              个句柄无法读取，{handles().timeouts} 次查询超时。
                              <Show when={handles().truncated}>已达到查询限制。</Show>
                            </p>
                          </Show>
                          <Show when={!handles().viaService && !handles().elevated}>
                            <p class="note">可在设置中启用特权服务，以检查更多系统进程。</p>
                          </Show>
                          <Show when={handles().error}>{(error) => <p class="note">{error()}</p>}</Show>
                        </>
                      )}
                    </Show>
                    <Show when={report().restartError}>{(error) => <p class="note">{error()}</p>}</Show>
                    <Show when={report().unchecked > 0}>
                      <p class="note">
                        另有 {report().unchecked} 个文件 Restart Manager
                        未能检查，可能与权限或文件状态变化有关。
                      </p>
                    </Show>
                    <Show when={report().truncated}>
                      <p class="note">
                        文件夹中的文件过多，仅检查了前 {report().filesChecked + report().unchecked} 个。
                      </p>
                    </Show>
                  </div>
                )}
              </Match>
            </Switch>
          </div>
        </div>
        <div class="dialog-footer">
          <span class="dialog-hint">句柄查询可发现当前目录占用；受保护进程仍可能无法检查。</span>
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
          <p>释放鼠标以查找占用这些文件的进程</p>
        </div>
      </div>
    </Show>
  );
}
