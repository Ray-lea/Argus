import { createEffect, createMemo, createResource, For, on, Show } from "solid-js";
import { copyText, endRow, endTree, PRIORITY_LABEL, priorityOf, revealInExplorer } from "../lib/actions";
import { fmtCount, fmtCpu, fmtCpuTime, fmtDateTime, fmtMem, fmtNet } from "../lib/format";
import { api } from "../lib/ipc";
import { EASE_OUT, play } from "../lib/motion";
import { type DisplayRow, groupKey, procKey } from "../lib/rows";
import { procHistoryOf, type Selection, selection, services, setSelectedKey, snapshot } from "../lib/store";
import type { Integrity, PriorityClass, ProcessRow } from "../lib/types";
import { CopyIcon, DismissIcon, FolderIcon, InfoIcon } from "./icons";
import ProcIcon from "./ProcIcon";
import { showService } from "./ServicesPage";
import Trace, { easedMax } from "./Trace";

const INTEGRITY: Record<Integrity, string> = {
  untrusted: "不受信任",
  low: "低",
  medium: "中",
  mediumPlus: "中高",
  high: "高（已提升）",
  system: "系统",
  protected: "受保护",
};

const ARCH = { x64: "64 位 (x64)", x86: "32 位 (x86)", arm64: "ARM64", arm: "ARM" } as const;

/** Rebuilds a DisplayRow for actions shared with the table and context menu. */
function asRow(sel: Selection, proc: ProcessRow): DisplayRow {
  return {
    key: sel.isGroup ? groupKey(proc.name) : procKey(proc),
    kind: sel.isGroup ? "group" : "process",
    depth: 0,
    proc,
    label: proc.description ?? proc.name,
    detail: null,
    count: sel.members.length,
    cpu: 0,
    mem: 0,
    io: 0,
    gpu: 0,
    gpuEngine: null,
    netSent: 0,
    netReceived: 0,
    threads: 0,
    handles: 0,
    expandable: false,
    expanded: false,
    suspended: false,
    efficiency: sel.members.filter((p) => p.efficiency).length,
  };
}

export default function DetailsPanel(props: { sel: Selection }) {
  // Refetch the slow details only when the selected process instance changes.
  const identity = createMemo(
    () => {
      const p = props.sel.proc;
      return p ? { pid: p.pid, path: p.path, key: procKey(p) } : null;
    },
    null,
    { equals: (a, b) => a?.key === b?.key },
  );
  const [details] = createResource(identity, (id) => api.details(id.pid, id.path));
  // Ignore the previous process's answer while the new one loads.
  const info = () => {
    const d = details();
    return d && d.pid === props.sel.proc?.pid ? d : undefined;
  };

  const parent = () => {
    const p = props.sel.proc;
    if (!p) return null;
    const parent = snapshot()?.processes.find((x) => x.pid === p.ppid);
    return parent && parent.createTime <= p.createTime ? parent : null;
  };

  const groupSum = (pick: (p: ProcessRow) => number) => props.sel.members.reduce((a, p) => a + pick(p), 0);
  const history = () => (props.sel.proc ? procHistoryOf(procKey(props.sel.proc)) : undefined);
  const memMax = easedMax(
    () => Math.max(...(history()?.mem ?? [1])) * 1.25,
    () => identity()?.key,
  );

  // Another process selected: its details settle in where the last ones were.
  // The selection object is rebuilt on every sample; only a different key
  // (memo-compared) means another process.
  let panel!: HTMLElement;
  const key = createMemo(() => props.sel.key);
  createEffect(
    on(
      key,
      () =>
        play(
          panel.querySelector(".details-scroll"),
          [
            { opacity: 0, transform: "translateY(6px)" },
            { opacity: 1, transform: "none" },
          ],
          { duration: 200, easing: EASE_OUT },
        ),
      { defer: true },
    ),
  );

  return (
    <aside ref={panel} class="details" aria-label="进程详细信息">
      <Show
        when={props.sel.proc}
        fallback={
          <div class="details-scroll">
            <div class="details-head">
              <InfoIcon size={32} />
              <div>
                <div class="details-title">进程已退出</div>
                <div class="details-subtitle">选择列表中的其他进程查看详细信息。</div>
              </div>
              <button
                type="button"
                class="btn ghost small icon"
                title="关闭"
                onClick={() => setSelectedKey(null)}
              >
                <DismissIcon size={14} />
              </button>
            </div>
          </div>
        }
      >
        {(proc) => (
          <>
            <div class="details-scroll">
              <div class="details-head">
                <ProcIcon proc={proc()} size={32} />
                <div>
                  <div class="details-title">{proc().description ?? proc().name}</div>
                  <div class="details-subtitle">
                    {proc().name}
                    <Show when={proc().company}>
                      <br />
                      {proc().company}
                    </Show>
                  </div>
                </div>
                <button
                  type="button"
                  class="btn ghost small icon"
                  title="关闭详细信息"
                  aria-label="关闭详细信息"
                  onClick={() => setSelectedKey(null)}
                >
                  <DismissIcon size={14} />
                </button>
              </div>

              <Show when={props.sel.isGroup}>
                <div class="note">
                  <InfoIcon />
                  <span>
                    此应用共有 {props.sel.members.length} 个进程，合计 CPU {fmtCpu(groupSum((p) => p.cpu))}
                    %，内存 {fmtMem(groupSum((p) => p.privateWs))}。下方显示最早启动的主进程。
                  </span>
                </div>
              </Show>

              <div class="mini-charts">
                <div class="mini-chart">
                  <div class="vital-label">CPU</div>
                  <div class="mini-value">{fmtCpu(proc().cpu)}%</div>
                  <Trace class="trace" data={() => history()?.cpu ?? []} max={() => 100} colorVar="--cpu" />
                </div>
                <div class="mini-chart">
                  <div class="vital-label">内存</div>
                  <div class="mini-value">{fmtMem(proc().privateWs)}</div>
                  <Trace class="trace" data={() => history()?.mem ?? []} max={memMax} colorVar="--mem" />
                </div>
              </div>

              <Show when={services().filter((sv) => sv.pid === proc().pid)}>
                {(hosted) => (
                  <Show when={hosted().length > 0}>
                    <div class="section-title">运行的服务（{hosted().length}）</div>
                    <div class="svc-list">
                      <For each={hosted()}>
                        {(sv) => (
                          <button
                            type="button"
                            class="svc-item"
                            title={sv.description ?? sv.displayName}
                            onClick={() => showService(sv.name)}
                          >
                            <span class="label">{sv.displayName}</span>
                            <span class="dim">{sv.name}</span>
                          </button>
                        )}
                      </For>
                    </div>
                  </Show>
                )}
              </Show>

              <div class="section-title">进程</div>
              <dl class="facts selectable">
                <dt>PID</dt>
                <dd>{proc().pid}</dd>
                <dt>父进程</dt>
                <dd>
                  <Show
                    when={parent()}
                    fallback={<span class="text-[var(--ink-3)]">无（PID {proc().ppid}）</span>}
                  >
                    {(pp) => (
                      <button type="button" class="link" onClick={() => setSelectedKey(procKey(pp()))}>
                        {pp().name}（{pp().pid}）
                      </button>
                    )}
                  </Show>
                </dd>
                <dt>启动时间</dt>
                <dd>{fmtDateTime(proc().createTime)}</dd>
                <dt>CPU 时间</dt>
                <dd>{fmtCpuTime(proc().cpuTime)}</dd>
                <dt>用户</dt>
                <dd>{info()?.user ?? "—"}</dd>
                <dt>完整性</dt>
                <dd>{info()?.integrity ? INTEGRITY[info()?.integrity as Integrity] : "—"}</dd>
                <dt>架构</dt>
                <dd>{info()?.architecture ? ARCH[info()?.architecture as keyof typeof ARCH] : "—"}</dd>
                <dt>优先级</dt>
                <dd>{PRIORITY_LABEL[priorityOf(proc().basePriority) as PriorityClass] ?? "—"}</dd>
                <dt>效率模式</dt>
                <dd>{proc().efficiency ? "开启" : "关闭"}</dd>
                <dt>会话</dt>
                <dd>{proc().sessionId}</dd>
              </dl>

              <div class="section-title">资源</div>
              <dl class="facts">
                <dt>专用工作集</dt>
                <dd>{fmtMem(proc().privateWs)}</dd>
                <dt>工作集</dt>
                <dd>{fmtMem(proc().workingSet)}</dd>
                <dt>提交大小</dt>
                <dd>{fmtMem(proc().commit)}</dd>
                <Show when={(snapshot()?.hardware.gpus.length ?? 0) > 0}>
                  <dt>GPU</dt>
                  <dd>
                    {fmtCpu(proc().gpu)}%
                    <Show when={proc().gpuEngine}>
                      <span class="dim">（{proc().gpuEngine}）</span>
                    </Show>
                  </dd>
                  <dt>专用 GPU 内存</dt>
                  <dd>{fmtMem(proc().gpuDedicated)}</dd>
                  <dt>共享 GPU 内存</dt>
                  <dd>{fmtMem(proc().gpuShared)}</dd>
                </Show>
                <dt>网络</dt>
                <dd>
                  <Show
                    when={snapshot()?.processNetwork}
                    fallback={<span class="dim">以管理员身份运行后可查看</span>}
                  >
                    发送 {fmtNet(proc().netSent)}，接收 {fmtNet(proc().netReceived)}
                  </Show>
                </dd>
                <dt>线程</dt>
                <dd>{fmtCount(proc().threads)}</dd>
                <dt>句柄</dt>
                <dd>{fmtCount(proc().handles)}</dd>
              </dl>

              <Show when={proc().path}>
                {(path) => (
                  <>
                    <div class="section-title">文件位置</div>
                    <div class="path-row">
                      <div class="code selectable">{path()}</div>
                      <div class="flex flex-col gap-1">
                        <button
                          type="button"
                          class="btn ghost small icon"
                          title="打开文件位置"
                          aria-label="打开文件位置"
                          onClick={() => revealInExplorer(path())}
                        >
                          <FolderIcon size={14} />
                        </button>
                        <button
                          type="button"
                          class="btn ghost small icon"
                          title="复制路径"
                          aria-label="复制路径"
                          onClick={() => copyText(path(), "路径")}
                        >
                          <CopyIcon size={14} />
                        </button>
                      </div>
                    </div>
                  </>
                )}
              </Show>

              <Show when={info()?.commandLine}>
                {(cmd) => (
                  <>
                    <div class="section-title">命令行</div>
                    <div class="code selectable">{cmd()}</div>
                  </>
                )}
              </Show>

              <Show when={info() && !info()?.accessible}>
                <div class="note">
                  <InfoIcon />
                  <span>无法打开此进程读取命令行和用户信息。以管理员身份运行 Argus 可以查看。</span>
                </div>
              </Show>

              <Show when={info()?.version.fileVersion || info()?.version.productName}>
                <div class="section-title">文件版本</div>
                <dl class="facts selectable">
                  <Show when={info()?.version.productName}>
                    <dt>产品</dt>
                    <dd>{info()?.version.productName}</dd>
                  </Show>
                  <Show when={info()?.version.fileVersion}>
                    <dt>版本</dt>
                    <dd>{info()?.version.fileVersion}</dd>
                  </Show>
                </dl>
              </Show>
            </div>

            <div class="details-actions">
              <button type="button" class="btn danger" onClick={() => endRow(asRow(props.sel, proc()))}>
                {props.sel.isGroup ? "全部结束" : "结束进程"}
              </button>
              <Show when={!props.sel.isGroup}>
                <button type="button" class="btn" onClick={() => endTree(asRow(props.sel, proc()))}>
                  结束进程树
                </button>
              </Show>
            </div>
          </>
        )}
      </Show>
    </aside>
  );
}

/** Wrapper that keeps the panel out of the layout until something is selected. */
export function DetailsHost(props: { open: boolean }) {
  return <Show when={props.open && selection()}>{(sel) => <DetailsPanel sel={sel()} />}</Show>;
}
