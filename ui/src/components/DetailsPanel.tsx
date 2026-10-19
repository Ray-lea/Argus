import { createEffect, createMemo, createResource, For, on, Show } from "solid-js";
import { copyText, endRow, endTree, PRIORITY_LABEL, priorityOf, revealInExplorer } from "../lib/actions";
import { fmtCount, fmtCpu, fmtCpuTime, fmtDateTime, fmtMem, fmtNet, fmtRate } from "../lib/format";
import { t } from "../lib/i18n";
import { api } from "../lib/ipc";
import { EASE_OUT, play } from "../lib/motion";
import { missingDiskHint } from "../lib/privilege";
import { sumDiskIo } from "../lib/process-io";
import { processTitle } from "../lib/process-label";
import { type DisplayRow, procKey } from "../lib/rows";
import {
  groupHistoryOf,
  procHistoryOf,
  revealProcess,
  type Selection,
  selection,
  services,
  setSelectedKey,
  snapshot,
} from "../lib/store";
import type { PriorityClass, ProcessDetails, ProcessRow } from "../lib/types";
import { CopyIcon, DismissIcon, FolderIcon, InfoIcon } from "./icons";
import ProcIcon from "./ProcIcon";
import { showService } from "./ServicesPage";
import Trace, { easedMax } from "./Trace";

const integrityName = (level: ProcessDetails["integrity"] | undefined) =>
  level ? t(`details.integrityLevel.${level}`) : "—";
const archName = (arch: ProcessDetails["architecture"] | undefined) =>
  arch ? t(`details.arch.${arch}`) : "—";

/** Rebuilds a DisplayRow for actions shared with the table and context menu. */
function asRow(sel: Selection, proc: ProcessRow): DisplayRow {
  return {
    key: sel.key,
    kind: sel.isGroup ? "group" : "process",
    depth: 0,
    proc,
    label: processTitle(proc),
    detail: null,
    count: sel.members.length,
    cpu: 0,
    mem: 0,
    io: 0,
    diskRead: proc.diskRead,
    diskWrite: proc.diskWrite,
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
    app: sel.members.some((p) => p.appWindow),
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

  // An app group shows its own totals, history and members; the details
  // fetched for its earliest process only supply what they share (version).
  const groupSum = (pick: (p: ProcessRow) => number) => props.sel.members.reduce((a, p) => a + pick(p), 0);
  const cpu = () => (props.sel.isGroup ? groupSum((p) => p.cpu) : (props.sel.proc?.cpu ?? 0));
  const mem = () => (props.sel.isGroup ? groupSum((p) => p.privateWs) : (props.sel.proc?.privateWs ?? 0));
  const history = () =>
    props.sel.isGroup
      ? groupHistoryOf(props.sel.key)
      : props.sel.proc
        ? procHistoryOf(procKey(props.sel.proc))
        : undefined;
  const hosted = () => {
    const pids = new Set(props.sel.members.map((p) => p.pid));
    return services().filter((sv) => pids.has(sv.pid));
  };
  const memMax = easedMax(
    () => Math.max(...(history()?.mem ?? [1])) * 1.25,
    () => props.sel.key,
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
    <aside ref={panel} class="details" aria-label={t("details.label")}>
      <Show
        when={props.sel.proc}
        fallback={
          <div class="details-scroll">
            <div class="details-head">
              <InfoIcon size={32} />
              <div>
                <div class="details-title">{t("details.exited.title")}</div>
                <div class="details-subtitle">{t("details.exited.hint")}</div>
              </div>
              <button
                type="button"
                class="btn ghost small icon"
                title={t("common.close")}
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
                  <div class="details-title">{processTitle(proc())}</div>
                  <div class="details-subtitle">
                    {proc().name}
                    <Show when={props.sel.isGroup}>
                      {" · "}
                      {t("details.group.count", { count: props.sel.members.length })}
                    </Show>
                    <Show when={proc().company}>
                      <br />
                      {proc().company}
                    </Show>
                  </div>
                </div>
                <button
                  type="button"
                  class="btn ghost small icon"
                  title={t("details.close")}
                  aria-label={t("details.close")}
                  onClick={() => setSelectedKey(null)}
                >
                  <DismissIcon size={14} />
                </button>
              </div>

              <div class="mini-charts">
                <div class="mini-chart">
                  <div class="vital-label">CPU</div>
                  <div class="mini-value">{fmtCpu(cpu())}%</div>
                  <Trace class="trace" data={() => history()?.cpu ?? []} max={() => 100} colorVar="--cpu" />
                </div>
                <div class="mini-chart">
                  <div class="vital-label">{t("common.resource.memory")}</div>
                  <div class="mini-value">{fmtMem(mem())}</div>
                  <Trace class="trace" data={() => history()?.mem ?? []} max={memMax} colorVar="--mem" />
                </div>
              </div>

              <Show when={hosted()}>
                {(hosted) => (
                  <Show when={hosted().length > 0}>
                    <div class="section-title">{t("details.services", { count: hosted().length })}</div>
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

              <Show when={props.sel.isGroup}>
                <GroupFacts members={props.sel.members} />
              </Show>

              <Show when={!props.sel.isGroup}>
                <div class="section-title">{t("details.section.process")}</div>
                <dl class="facts selectable">
                  <dt>PID</dt>
                  <dd>{proc().pid}</dd>
                  <dt>{t("details.parent")}</dt>
                  <dd>
                    <Show
                      when={parent()}
                      fallback={
                        <span class="text-[var(--ink-3)]">{t("details.noParent", { pid: proc().ppid })}</span>
                      }
                    >
                      {(pp) => (
                        <button type="button" class="link" onClick={() => setSelectedKey(procKey(pp()))}>
                          {t("details.parentLink", { name: pp().name, pid: pp().pid })}
                        </button>
                      )}
                    </Show>
                  </dd>
                  <dt>{t("details.started")}</dt>
                  <dd>{fmtDateTime(proc().createTime)}</dd>
                  <dt>{t("details.cpuTime")}</dt>
                  <dd>{fmtCpuTime(proc().cpuTime)}</dd>
                  <dt>{t("details.user")}</dt>
                  <dd>{info()?.user ?? "—"}</dd>
                  <dt>{t("details.integrity")}</dt>
                  <dd>{integrityName(info()?.integrity)}</dd>
                  <dt>{t("details.architecture")}</dt>
                  <dd>{archName(info()?.architecture)}</dd>
                  <dt>{t("details.priority")}</dt>
                  <dd>{PRIORITY_LABEL[priorityOf(proc().basePriority) as PriorityClass] ?? "—"}</dd>
                  <dt>{t("common.efficiencyMode")}</dt>
                  <dd>{proc().efficiency ? t("common.on") : t("common.off")}</dd>
                  <dt>{t("details.session")}</dt>
                  <dd>{proc().sessionId}</dd>
                </dl>

                <div class="section-title">{t("details.section.resources")}</div>
                <dl class="facts">
                  <dt>{t("details.privateWs")}</dt>
                  <dd>{fmtMem(proc().privateWs)}</dd>
                  <dt>{t("details.workingSet")}</dt>
                  <dd>{fmtMem(proc().workingSet)}</dd>
                  <dt>{t("details.commit")}</dt>
                  <dd>{fmtMem(proc().commit)}</dd>
                  <DiskFacts members={[proc()]} />
                  <Show when={(snapshot()?.hardware.gpus.length ?? 0) > 0}>
                    <dt>GPU</dt>
                    <dd>
                      {fmtCpu(proc().gpu)}%
                      <Show when={proc().gpuEngine}>
                        <span class="dim">{t("details.gpuEngine", { engine: proc().gpuEngine ?? "" })}</span>
                      </Show>
                    </dd>
                    <dt>{t("details.gpuDedicated")}</dt>
                    <dd>{fmtMem(proc().gpuDedicated)}</dd>
                    <dt>{t("details.gpuShared")}</dt>
                    <dd>{fmtMem(proc().gpuShared)}</dd>
                  </Show>
                  <dt>{t("common.resource.network")}</dt>
                  <dd>
                    <Show
                      when={snapshot()?.processNetwork}
                      fallback={<span class="dim">{t("details.netNeedsAdmin")}</span>}
                    >
                      {t("processes.netRates", {
                        sent: fmtNet(proc().netSent),
                        received: fmtNet(proc().netReceived),
                      })}
                    </Show>
                  </dd>
                  <dt>{t("details.threads")}</dt>
                  <dd>{fmtCount(proc().threads)}</dd>
                  <dt>{t("details.handles")}</dt>
                  <dd>{fmtCount(proc().handles)}</dd>
                </dl>
              </Show>

              <Show when={!props.sel.isGroup && proc().path}>
                {(path) => (
                  <>
                    <div class="section-title">{t("details.section.location")}</div>
                    <div class="path-row">
                      <div class="code selectable">{path()}</div>
                      <div class="flex flex-col gap-1">
                        <button
                          type="button"
                          class="btn ghost small icon"
                          title={t("common.openFileLocation")}
                          aria-label={t("common.openFileLocation")}
                          onClick={() => revealInExplorer(path())}
                        >
                          <FolderIcon size={14} />
                        </button>
                        <button
                          type="button"
                          class="btn ghost small icon"
                          title={t("common.copyPath")}
                          aria-label={t("common.copyPath")}
                          onClick={() => copyText(path(), t("actions.copyWhat.path"))}
                        >
                          <CopyIcon size={14} />
                        </button>
                      </div>
                    </div>
                  </>
                )}
              </Show>

              <Show when={!props.sel.isGroup && info()?.commandLine}>
                {(cmd) => (
                  <>
                    <div class="section-title">{t("details.section.commandLine")}</div>
                    <div class="code selectable">{cmd()}</div>
                  </>
                )}
              </Show>

              <Show when={!props.sel.isGroup && info() && !info()?.accessible}>
                <div class="note">
                  <InfoIcon />
                  <span>{t("details.noAccess")}</span>
                </div>
              </Show>

              <Show when={info()?.version.fileVersion || info()?.version.productName}>
                <div class="section-title">{t("details.section.version")}</div>
                <dl class="facts selectable">
                  <Show when={info()?.version.productName}>
                    <dt>{t("details.product")}</dt>
                    <dd>{info()?.version.productName}</dd>
                  </Show>
                  <Show when={info()?.version.fileVersion}>
                    <dt>{t("details.version")}</dt>
                    <dd>{info()?.version.fileVersion}</dd>
                  </Show>
                </dl>
              </Show>
            </div>

            <div class="details-actions">
              <button type="button" class="btn danger" onClick={() => endRow(asRow(props.sel, proc()))}>
                {props.sel.isGroup ? t("actions.endAll") : t("common.endProcess")}
              </button>
              <Show when={!props.sel.isGroup}>
                <button type="button" class="btn" onClick={() => endTree(asRow(props.sel, proc()))}>
                  {t("common.endProcessTree")}
                </button>
              </Show>
            </div>
          </>
        )}
      </Show>
    </aside>
  );
}

/** Disk read and write rates of a process, or the measured members of a group. */
function DiskFacts(props: { members: ProcessRow[] }) {
  const rates = createMemo(() => sumDiskIo(props.members));
  const missing = createMemo(missingDiskHint);
  return (
    <For each={["diskRead", "diskWrite"] as const}>
      {(key) => (
        <>
          <dt>{t(`processes.column.${key}.label`)}</dt>
          <dd title={rates()[key] == null ? missing() : t(`processes.column.${key}.title`)}>
            {rates()[key] == null ? "—" : fmtRate(rates()[key] ?? 0, false)}
          </dd>
        </>
      )}
    </For>
  );
}

/** An app group: its resources added up, its processes, and where they run from. */
function GroupFacts(props: { members: ProcessRow[] }) {
  const sum = (pick: (p: ProcessRow) => number) => props.members.reduce((a, p) => a + pick(p), 0);
  const earliest = () => Math.min(...props.members.map((p) => p.createTime));
  const busiest = () => props.members.reduce((a, p) => (p.gpu > a.gpu ? p : a), props.members[0]);
  // Busiest first; a process clicked here opens on its own.
  const byLoad = () => [...props.members].sort((a, b) => b.cpu - a.cpu || b.privateWs - a.privateWs);
  const paths = () => [...new Set(props.members.map((p) => p.path).filter((p): p is string => !!p))];
  return (
    <>
      <div class="section-title">{t("details.section.resources")}</div>
      <dl class="facts">
        <dt>{t("details.privateWs")}</dt>
        <dd>{fmtMem(sum((p) => p.privateWs))}</dd>
        <dt>{t("details.workingSet")}</dt>
        <dd>{fmtMem(sum((p) => p.workingSet))}</dd>
        <dt>{t("details.commit")}</dt>
        <dd>{fmtMem(sum((p) => p.commit))}</dd>
        <DiskFacts members={props.members} />
        <Show when={(snapshot()?.hardware.gpus.length ?? 0) > 0}>
          <dt>GPU</dt>
          <dd>
            {fmtCpu(
              Math.min(
                100,
                sum((p) => p.gpu),
              ),
            )}
            %
            <Show when={busiest()?.gpu && busiest()?.gpuEngine}>
              <span class="dim">{t("details.gpuEngine", { engine: busiest()?.gpuEngine ?? "" })}</span>
            </Show>
          </dd>
          <dt>{t("details.gpuDedicated")}</dt>
          <dd>{fmtMem(sum((p) => p.gpuDedicated))}</dd>
          <dt>{t("details.gpuShared")}</dt>
          <dd>{fmtMem(sum((p) => p.gpuShared))}</dd>
        </Show>
        <dt>{t("common.resource.network")}</dt>
        <dd>
          <Show
            when={snapshot()?.processNetwork}
            fallback={<span class="dim">{t("details.netNeedsAdmin")}</span>}
          >
            {t("processes.netRates", {
              sent: fmtNet(sum((p) => p.netSent)),
              received: fmtNet(sum((p) => p.netReceived)),
            })}
          </Show>
        </dd>
        <dt>{t("details.threads")}</dt>
        <dd>{fmtCount(sum((p) => p.threads))}</dd>
        <dt>{t("details.handles")}</dt>
        <dd>{fmtCount(sum((p) => p.handles))}</dd>
        <dt>{t("details.cpuTime")}</dt>
        <dd>{fmtCpuTime(sum((p) => p.cpuTime))}</dd>
        <dt>{t("details.group.earliest")}</dt>
        <dd>{fmtDateTime(earliest())}</dd>
      </dl>

      <div class="section-title">{t("details.group.members", { count: props.members.length })}</div>
      <div class="member-list">
        <For each={byLoad()}>
          {(p) => (
            <button
              type="button"
              class="member"
              title={t("details.group.open", { pid: p.pid })}
              onClick={() => revealProcess(p.pid)}
            >
              <span class="member-pid">PID {p.pid}</span>
              <span class="member-value">{fmtCpu(p.cpu)}%</span>
              <span class="member-value">{fmtMem(p.privateWs)}</span>
            </button>
          )}
        </For>
      </div>

      <Show when={paths().length > 0}>
        <div class="section-title">{t("details.section.location")}</div>
        <For each={paths()}>
          {(path) => (
            <div class="path-row">
              <div class="code selectable">{path}</div>
              <div class="flex flex-col gap-1">
                <button
                  type="button"
                  class="btn ghost small icon"
                  title={t("common.openFileLocation")}
                  aria-label={t("common.openFileLocation")}
                  onClick={() => revealInExplorer(path)}
                >
                  <FolderIcon size={14} />
                </button>
                <button
                  type="button"
                  class="btn ghost small icon"
                  title={t("common.copyPath")}
                  aria-label={t("common.copyPath")}
                  onClick={() => copyText(path, t("actions.copyWhat.path"))}
                >
                  <CopyIcon size={14} />
                </button>
              </div>
            </div>
          )}
        </For>
      </Show>
    </>
  );
}

/** Wrapper that keeps the panel out of the layout until something is selected. */
export function DetailsHost(props: { open: boolean }) {
  return <Show when={props.open && selection()}>{(sel) => <DetailsPanel sel={sel()} />}</Show>;
}
