/** @jsxImportSource @/components/replica/i18n */
"use client";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent,
  type ReactNode,
} from "react";
import { Icon, ProcessIcon, type UiIcon } from "./icons";
import type { Motion } from "./Curve";
import { language, translate } from "./i18n";
import { dateLocale, isLanguage, languages, localized, preferredLanguage, rememberLanguage, type Language } from "@/lib/languages";
import {
  appVersion,
  bytes,
  connections,
  noise,
  percent,
  defaultVisibility,
  processes,
  programNames,
  rate,
  rateLabel,
  startLabel,
  stateLabel,
  type Process,
  type Service,
  type StartupItem,
  type VisibilityRules,
  historyRanges,
  type HistoryRange,
} from "./model";
import { Button, scaleOf, Segments, Select, Switch } from "./widgets";

export type ServiceFilter = "all" | "running" | "stopped";
export type ServiceState = "running" | "stopped" | "startPending" | "stopPending";
const stateText: Record<ServiceState, string> = {
  running: "正在运行",
  stopped: "已停止",
  startPending: "正在启动",
  stopPending: "正在停止",
};
export type MenuEntry =
  | "-"
  | {
      label: string;
      icon?: UiIcon;
      hint?: string;
      danger?: boolean;
      disabled?: boolean;
      checked?: boolean;
      children?: MenuEntry[];
      /** Quiet text at the end of the item, such as a submenu's checked count. */
      detail?: string;
      /** A check toggled in place, the menu staying open. */
      stay?: boolean;
      run?: () => void;
    };
type OpenMenu = (event: MouseEvent, items: MenuEntry[]) => void;

// Sortable headers of the list pages (34 px cells; the sorted one bold with its chevron).
interface Col<K extends string> {
  key: K;
  label: string;
  end?: boolean;
  className?: string;
}
function useSort<K extends string>(initial: K) {
  const [state, setState] = useState<{ key: K; desc: boolean }>({ key: initial, desc: false });
  const toggle = (key: K) => setState((s) => (s.key === key ? { key, desc: !s.desc } : { key, desc: false }));
  return [state, toggle] as const;
}
function Header<K extends string>({
  cols,
  sort,
  onSort,
}: {
  cols: Col<K>[];
  sort: { key: K; desc: boolean };
  onSort: (key: K) => void;
}) {
  return (
    <div className="a-thead" role="row">
      {cols.map((c) => (
        <button
          key={c.key}
          type="button"
          role="columnheader"
          className={"a-th" + (c.end ? " is-end" : "") + (c.className ? " " + c.className : "")}
          aria-sort={sort.key === c.key ? (sort.desc ? "descending" : "ascending") : "none"}
          onClick={() => onSort(c.key)}
        >
          <span className="a-th-label">
            <span>{c.label}</span>
            {sort.key === c.key && <Icon name={sort.desc ? "chevronDown" : "chevronUp"} size={12} />}
          </span>
        </button>
      ))}
    </div>
  );
}
function sorted<T>(rows: T[], value: (row: T) => string | number, desc: boolean) {
  // Text sorts as it reads in the page's language.
  const collator = new Intl.Collator(dateLocale(language()), { numeric: true });
  return [...rows].sort((a, b) => {
    const x = value(a);
    const y = value(b);
    const c = typeof x === "number" && typeof y === "number" ? x - y : collator.compare(translate(String(x)), translate(String(y)));
    return desc ? -c : c;
  });
}

type ServiceCol = "display" | "name" | "pid" | "state" | "start" | "description";
export function Services({
  list,
  states,
  filter,
  query,
  selected,
  onSelect,
  onMenu,
}: {
  list: Service[];
  states: Map<string, ServiceState>;
  filter: ServiceFilter;
  query: string;
  selected: string | null;
  onSelect: (name: string | null) => void;
  onMenu: (event: MouseEvent, service: Service) => void;
}) {
  const [sort, setSort] = useSort<ServiceCol>("display");
  const q = query.trim().toLowerCase();
  const rows = sorted(
    list.filter((s) => {
      const state = states.get(s.name)!;
      if (filter === "running" && state !== "running") return false;
      if (filter === "stopped" && state !== "stopped") return false;
      return (
        !q ||
        s.display.toLowerCase().includes(q) ||
        s.name.toLowerCase().includes(q) ||
        s.description.includes(q) ||
        String(s.pid ?? "") === q
      );
    }),
    (s) =>
      sort.key === "pid"
        ? states.get(s.name) === "running"
          ? (s.pid ?? 0)
          : 0
        : sort.key === "state"
          ? stateText[states.get(s.name)!]
          : sort.key === "start"
            ? startLabel[s.start]
            : sort.key === "name"
              ? s.name
              : sort.key === "description"
                ? s.description
                : s.display,
    sort.desc,
  );
  const running = list.filter((s) => states.get(s.name) === "running").length;
  return (
    <div className="a-page">
      <div className="a-list-card">
        <div className="a-table a-services" role="grid" aria-label="服务">
          <Header
            sort={sort}
            onSort={setSort}
            cols={[
              { key: "display", label: "服务" },
              { key: "name", label: "名称", className: "a-c-name" },
              { key: "pid", label: "PID", end: true, className: "a-c-pid" },
              { key: "state", label: "状态" },
              { key: "start", label: "启动类型" },
              { key: "description", label: "描述", className: "a-col-wide" },
            ]}
          />
          <div className="a-tbody a-scroll" role="rowgroup" tabIndex={0}>
            {rows.length === 0 && <p className="a-empty">没有匹配的服务</p>}
            {rows.map((s) => {
              const state = states.get(s.name)!;
              return (
                <div
                  key={s.name}
                  role="row"
                  aria-selected={selected === s.name}
                  className={"a-tr" + (selected === s.name ? " is-selected" : "")}
                  onClick={() => onSelect(selected === s.name ? null : s.name)}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    onSelect(s.name);
                    onMenu(event, s);
                  }}
                >
                  <span role="gridcell" className="a-td" title={s.display}>
                    <span className="a-cell-text">{s.display}</span>
                  </span>
                  <span role="gridcell" className="a-td is-faint a-c-name">
                    <span className="a-cell-text">{s.name}</span>
                  </span>
                  <span role="gridcell" className="a-td is-end is-faint a-c-pid">
                    {state === "running" ? (s.pid ?? "") : ""}
                  </span>
                  <span role="gridcell" className="a-td">
                    <i className={"a-dot" + (state === "running" ? " is-ok" : state.endsWith("Pending") ? " is-busy" : "")} />
                    {stateText[state]}
                  </span>
                  <span
                    role="gridcell"
                    className="a-td"
                    style={s.start === "disabled" ? { color: "var(--a-io)" } : s.start === "manual" ? { color: "var(--a-ink-3)" } : undefined}
                  >
                    {startLabel[s.start]}
                  </span>
                  <span role="gridcell" className="a-td is-faint a-col-wide" title={s.description}>
                    <span className="a-cell-text">{s.description}</span>
                  </span>
                </div>
              );
            })}
          </div>
        </div>
        <footer className="a-list-foot">
          <span>
            {`正在运行 ${running} / 共 ${list.length} 个服务`}
          </span>
        </footer>
      </div>
    </div>
  );
}

type StartupCol = "name" | "publisher" | "enabled" | "location" | "command";
export function Startup({
  items,
  query,
  onToggle,
  onMenu,
}: {
  items: StartupItem[];
  query: string;
  onToggle: (name: string) => void;
  onMenu: OpenMenu;
}) {
  const [sort, setSort] = useSort<StartupCol>("name");
  const [selected, setSelected] = useState<string | null>(null);
  const q = query.trim().toLowerCase();
  const rows = sorted(
    items.filter(
      (i) => !q || i.name.toLowerCase().includes(q) || i.publisher.toLowerCase().includes(q) || i.command.toLowerCase().includes(q),
    ),
    (i) => (sort.key === "enabled" ? (i.enabled ? 1 : 0) : i[sort.key]),
    sort.desc,
  );
  const enabled = items.filter((i) => i.enabled).length;
  return (
    <div className="a-page">
      <div className="a-list-card">
        <div className="a-table a-startup" role="grid" aria-label="启动项">
          <Header
            sort={sort}
            onSort={setSort}
            cols={[
              { key: "name", label: "名称" },
              { key: "publisher", label: "发布者", className: "a-c-publisher" },
              { key: "enabled", label: "状态" },
              { key: "location", label: "位置", className: "a-c-location" },
              { key: "command", label: "命令", className: "a-col-wide" },
            ]}
          />
          <div className="a-tbody a-scroll" role="rowgroup" tabIndex={0}>
            {rows.length === 0 && <p className="a-empty">没有匹配的启动项</p>}
            {rows.map((item) => (
              <div
                key={item.name}
                role="row"
                aria-selected={selected === item.name}
                className={"a-tr" + (selected === item.name ? " is-selected" : "")}
                onClick={() => setSelected(selected === item.name ? null : item.name)}
                onContextMenu={(event) => {
                  event.preventDefault();
                  setSelected(item.name);
                  onMenu(event, [
                    { label: item.enabled ? "禁用" : "启用", run: () => onToggle(item.name) },
                    "-",
                    { label: "打开文件位置", icon: "folder", disabled: item.missing },
                    { label: "复制命令", icon: "copy", run: () => void navigator.clipboard?.writeText(item.command).catch(() => {}) },
                    "-",
                    { label: "置顶", icon: "pin" },
                    { label: "添加备注", icon: "note" },
                  ]);
                }}
              >
                <span role="gridcell" className="a-td" title={item.name + "\n" + item.command}>
                  <ProcessIcon glyph={item.glyph} />
                  <span className={"a-cell-text" + (item.enabled ? "" : " is-faint")} style={item.enabled ? undefined : { color: "var(--a-ink-2)" }}>
                    {item.name}
                  </span>
                  {item.missing && (
                    <span className="a-tag is-warn" title="命令指向的程序已不存在，可禁用此项">
                      文件不存在
                    </span>
                  )}
                </span>
                <span role="gridcell" className="a-td is-faint a-c-publisher">
                  <span className="a-cell-text">{item.publisher}</span>
                </span>
                <span role="gridcell" className="a-td" onClick={(event) => event.stopPropagation()}>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={item.enabled}
                    data-startup={item.name}
                    aria-label={(item.enabled ? "禁用 " : "启用 ") + item.name}
                    className="a-switch is-small"
                    onClick={() => onToggle(item.name)}
                  >
                    <i />
                  </button>
                  <span style={item.enabled ? undefined : { color: "var(--a-ink-3)" }}>{item.enabled ? "已启用" : "已禁用"}</span>
                </span>
                <span role="gridcell" className="a-td is-faint a-c-location">
                  <span className="a-cell-text">{item.location}</span>
                </span>
                <span role="gridcell" className="a-td is-faint a-mono a-col-wide" title={item.command}>
                  <span className="a-cell-text">{item.command}</span>
                </span>
              </div>
            ))}
          </div>
        </div>
        <footer className="a-list-foot">
          <span>
            {`已启用 ${enabled} / 共 ${items.length} 项`}
          </span>
          <span>禁用操作与任务管理器的“启动应用”使用同一开关，可重新启用；“所有用户”的项需要管理员权限。</span>
        </footer>
      </div>
    </div>
  );
}

export type ConnectionFilter = "all" | "listen" | "established" | "udp";
type ConnectionCol = "name" | "pid" | "protocol" | "local" | "localPort" | "remote" | "remotePort" | "state";
export function Connections({
  alive,
  filter,
  query,
  hidden,
  onShowAll,
  onOpen,
  onMenu,
}: {
  alive: Process[];
  filter: ConnectionFilter;
  query: string;
  /** While system processes are hidden: whether a process is one of them. */
  hidden?: (proc: Process) => boolean;
  onShowAll: () => void;
  onOpen: (pid: number) => void;
  onMenu: OpenMenu;
}) {
  const [sort, setSort] = useSort<ConnectionCol>("name");
  const [selected, setSelected] = useState<number | null>(null);
  const byPid = new Map(processes.map((p) => [p.pid, p]));
  const living = new Set(alive.map((p) => p.pid));
  const q = query.trim();
  const port = /^\d{1,5}$/.test(q) ? Number(q) : null;
  const visible = connections
    .map((c, index) => ({ ...c, index, proc: byPid.get(c.pid) }))
    .filter((c) => {
      // Hidden with their processes, and the records no process owns (PID 0).
      if (hidden && (c.pid === 0 || (c.proc && hidden(c.proc)))) return false;
      if (filter === "listen" && c.state !== "listen") return false;
      if (filter === "established" && c.state !== "established") return false;
      if (filter === "udp" && c.protocol !== "UDP") return false;
      if (port !== null) return c.localPort === port || c.remotePort === port || String(c.pid).startsWith(q);
      if (!q) return true;
      const needle = q.toLowerCase();
      return (
        (c.proc?.exe.toLowerCase().includes(needle) ?? false) ||
        (c.proc?.name.toLowerCase().includes(needle) ?? false) ||
        c.local.includes(q) ||
        (c.remote?.includes(q) ?? false)
      );
    });
  const rows = sorted(
    visible,
    (c) =>
      sort.key === "name"
        ? (c.proc?.exe ?? "")
        : sort.key === "pid"
          ? c.pid
          : sort.key === "protocol"
            ? c.protocol
            : sort.key === "local"
              ? c.local
              : sort.key === "localPort"
                ? c.localPort
                : sort.key === "remote"
                  ? (c.remote ?? "")
                  : sort.key === "remotePort"
                    ? (c.remotePort ?? 0)
                    : (c.state ?? ""),
    sort.desc,
  );
  const listen = connections.filter((c) => c.state === "listen").length;
  const established = connections.filter((c) => c.state === "established").length;
  const udp = connections.filter((c) => c.protocol === "UDP").length;
  const endpoint = (address: string, p?: number) => (address.includes(":") ? `[${address}]:${p}` : `${address}:${p}`);
  return (
    <div className="a-page">
      <div className="a-list-card">
        <div className="a-table a-connections" role="grid" aria-label="网络连接">
          <Header
            sort={sort}
            onSort={setSort}
            cols={[
              { key: "name", label: "进程" },
              { key: "pid", label: "PID", end: true, className: "a-c-pid" },
              { key: "protocol", label: "协议" },
              { key: "local", label: "本地地址", className: "a-c-local" },
              { key: "localPort", label: "本地端口", end: true },
              { key: "remote", label: "远程地址", className: "a-c-remote" },
              { key: "remotePort", label: "远程端口", end: true, className: "a-c-rport" },
              { key: "state", label: "状态" },
            ]}
          />
          <div className="a-tbody a-scroll" role="rowgroup" tabIndex={0}>
            {rows.length === 0 &&
              (hidden ? (
                <div className="a-empty">
                  <span>当前筛选条件下没有可显示的项目。</span>
                  <button type="button" className="a-btn" onClick={onShowAll}>
                    显示全部并清除筛选
                  </button>
                </div>
              ) : (
                <p className="a-empty">{port !== null ? `没有匹配的连接：没有程序在使用端口 ${port}` : "没有匹配的连接"}</p>
              ))}
            {rows.map((c) => {
              const gone = !living.has(c.pid);
              return (
                <div
                  key={c.index}
                  role="row"
                  aria-selected={selected === c.index}
                  className={"a-tr" + (selected === c.index ? " is-selected" : "")}
                  title="双击转到对应进程"
                  onClick={() => setSelected(selected === c.index ? null : c.index)}
                  onDoubleClick={() => !gone && onOpen(c.pid)}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    setSelected(c.index);
                    onMenu(event, [
                      { label: "转到进程", icon: "goTo", disabled: gone, run: () => onOpen(c.pid) },
                      { label: `结束 ${c.proc?.name ?? "PID " + c.pid}`, icon: "end", danger: true, disabled: gone || c.pid <= 4 },
                      { label: "断开此连接", icon: "dismiss", disabled: c.protocol !== "TCP" || c.state !== "established" || c.local.includes(":") },
                      "-",
                      { label: "复制本地地址", icon: "copy", run: () => void navigator.clipboard?.writeText(endpoint(c.local, c.localPort)).catch(() => {}) },
                      { label: "复制远程地址", disabled: !c.remote, run: () => void navigator.clipboard?.writeText(endpoint(c.remote ?? "", c.remotePort)).catch(() => {}) },
                      "-",
                      { label: "置顶", icon: "pin" },
                      { label: "添加备注", icon: "note" },
                    ]);
                  }}
                >
                  <span role="gridcell" className="a-td" title={c.proc?.path}>
                    <ProcessIcon glyph={c.proc?.glyph ?? "app"} />
                    <span className="a-cell-text">{c.proc?.exe ?? "PID " + c.pid}</span>
                    {gone && <span className="a-tag">已退出</span>}
                  </span>
                  <span role="gridcell" className="a-td is-end is-faint a-c-pid">
                    {c.pid}
                  </span>
                  <span role="gridcell" className="a-td">
                    {c.protocol}
                    {c.local.includes(":") && <span className="a-tag">v6</span>}
                  </span>
                  <span role="gridcell" className="a-td a-mono a-c-local">
                    <span className="a-cell-text">{c.local}</span>
                  </span>
                  <span role="gridcell" className={"a-td is-end a-mono" + (port === c.localPort ? " a-hit" : "")}>
                    {c.localPort}
                  </span>
                  <span role="gridcell" className="a-td is-faint a-mono a-c-remote">
                    <span className="a-cell-text">{c.remote ?? ""}</span>
                  </span>
                  <span role="gridcell" className={"a-td is-end a-mono a-c-rport" + (port === c.remotePort ? " a-hit" : " is-faint")}>
                    {c.remotePort ?? ""}
                  </span>
                  <span role="gridcell" className="a-td">
                    <i className={"a-dot" + (c.state === "established" ? " is-ok" : c.state === "listen" ? " is-listen" : "")} />
                    {c.state ? stateLabel[c.state] : ""}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
        <footer className="a-list-foot">
          <span>
            监听 {listen} · 已连接 {established} · UDP {udp}
          </span>
          <span>每 2 秒刷新一次。双击某行可转到对应进程。</span>
        </footer>
      </div>
    </div>
  );
}

// History: one recorded series per resource over the chosen range, every
// strip on a shared time axis; a click picks a moment for the table below.
export type { HistoryRange };
const POINTS = 240;
// The demo's clock: Tuesday 3 November, 21:36.
const NOW_MINUTE = 21 * 60 + 36;
function moment(range: HistoryRange, index: number) {
  const r = historyRanges.find((x) => x.value === range)!;
  const minutesAgo = ((POINTS - 1 - index) / (POINTS - 1)) * r.hours * 60;
  const total = NOW_MINUTE - minutesAgo;
  const day = Math.floor(total / 1440);
  const inDay = ((total % 1440) + 1440) % 1440;
  const seconds = Math.round((inDay % 1) * 60);
  const pad = (v: number) => String(Math.floor(v)).padStart(2, "0");
  return { day: 3 + day, text: `11月${3 + day}日 ${pad(inDay / 60)}:${pad(inDay % 60)}:${pad(seconds)}` };
}
function historySeries(range: HistoryRange) {
  const r = historyRanges.find((x) => x.value === range)!;
  const seed = r.hours;
  const wave = (i: number, k: number) => {
    const minutes = NOW_MINUTE - ((POINTS - 1 - i) / (POINTS - 1)) * r.hours * 60;
    const hour = (((minutes / 60) % 24) + 24) % 24;
    const busy = hour >= 9 && hour <= 23 ? 1 : 0.25;
    return busy * (0.55 + 0.45 * Math.sin(i / 9 + k)) + noise(i * 7 + k, seed) * 0.35;
  };
  const spike = (i: number, k: number) => (noise(i + k * 31, seed + 5) > 0.93 ? 1 + noise(i, k) * 2 : 0);
  const cpu = Array.from({ length: POINTS }, (_, i) => Math.min(96, 6 + wave(i, 1) * 18 + spike(i, 1) * 20));
  return {
    cpu,
    cpuPeak: cpu.map((v, i) => Math.min(100, v * (1.35 + noise(i, 4) * 0.5))),
    mem: Array.from({ length: POINTS }, (_, i) => 36 + wave(i, 2) * 6 + Math.sin(i / 40) * 2),
    disk: Array.from({ length: POINTS }, (_, i) => (0.3 + wave(i, 3) * 1.4 + spike(i, 3) * 4) * 1024 * 1024),
    net: Array.from({ length: POINTS }, (_, i) => (0.2 + wave(i, 5) * 0.9 + spike(i, 5) * 2.4) * 1024 * 1024),
    gpu: Array.from({ length: POINTS }, (_, i) => Math.min(100, 3 + wave(i, 6) * 12 + spike(i, 6) * 18)),
    bat: Array.from({ length: POINTS }, (_, i) => Math.max(20, 96 - (i / POINTS) * (r.hours > 6 ? 40 : 18))),
  };
}
function linePath(values: number[], max: number) {
  const n = values.length;
  const x = (i: number) => ((i / (n - 1)) * 1000).toFixed(1);
  const y = (v: number) => (100 - (Math.max(0, Math.min(max, v)) / max) * 96).toFixed(2);
  return values.map((v, i) => `${i ? "L" : "M"}${x(i)} ${y(v)}`).join("");
}
export function History({
  range,
  cursor,
  onCursor,
  onOpen,
}: {
  range: HistoryRange;
  cursor: number | null;
  onCursor: (index: number | null) => void;
  onOpen: (pid: number) => void;
}) {
  const data = useMemo(() => historySeries(range), [range]);
  // The strips are static per range: their paths are drawn once, not on every sample or pointer move.
  const cache = useMemo(() => new Map<number[], string>(), [data]);
  const path = (values: number[], max: number) => {
    let d = cache.get(values);
    if (d === undefined) cache.set(values, (d = linePath(values, max)));
    return d;
  };
  const [hover, setHover] = useState<number | null>(null);
  const r = historyRanges.find((x) => x.value === range)!;
  const at = cursor ?? POINTS - 1;
  const diskTop = Math.max(...data.disk) > 8 * 1024 ** 2 ? 10 * 1024 ** 2 : 9 * 1024 ** 2;
  const netTop = Math.max(...data.net) > 3 * 1024 ** 2 ? 5 * 1024 ** 2 : 3 * 1024 ** 2;
  const metrics: {
    key: string;
    name: string;
    color: string;
    value: string;
    sub?: string;
    lines: { values: number[]; faint?: boolean }[];
    max: number;
    scale?: string;
  }[] = [
    {
      key: "cpu",
      name: "CPU",
      color: "var(--a-cpu)",
      value: `${data.cpu[at].toFixed(1)}%`,
      sub: `最高 ${data.cpuPeak[at].toFixed(1)}%`,
      lines: [{ values: data.cpuPeak, faint: true }, { values: data.cpu }],
      max: 100,
    },
    {
      key: "mem",
      name: "内存",
      color: "var(--a-mem)",
      value: `${((data.mem[at] / 100) * 32).toFixed(1)} GB`,
      sub: "共 32.0 GB",
      lines: [{ values: data.mem }],
      max: 100,
    },
    { key: "disk", name: "磁盘", color: "var(--a-io)", value: rate(data.disk[at]), lines: [{ values: data.disk }], max: diskTop, scale: rateLabel(diskTop) },
    { key: "net", name: "网络", color: "var(--a-net)", value: rate(data.net[at]), lines: [{ values: data.net }], max: netTop, scale: rateLabel(netTop) },
    { key: "gpu", name: "GPU", color: "var(--a-gpu)", value: `${Math.round(data.gpu[at])}%`, lines: [{ values: data.gpu }], max: 100 },
    { key: "bat", name: "电池", color: "var(--a-bat)", value: `${Math.round(data.bat[at])}%`, sub: "放电 7.8 W", lines: [{ values: data.bat }], max: 100 },
  ];
  // Tick marks on whole steps of the range's interval, as the app places them.
  const minutesSpan = r.hours * 60;
  const start = NOW_MINUTE - minutesSpan;
  const first = Math.ceil(start / r.tick) * r.tick;
  const ticks: { left: number; text: string }[] = [];
  for (let m = first; m <= NOW_MINUTE; m += r.tick) {
    const inDay = ((m % 1440) + 1440) % 1440;
    const left = ((m - start) / minutesSpan) * 100;
    if (left < 3 || left > 97) continue;
    ticks.push({
      left,
      text:
        r.tick >= 1440
          ? `11月${3 + Math.floor(m / 1440)}日`
          : `${String(Math.floor(inDay / 60)).padStart(2, "0")}:${String(inDay % 60).padStart(2, "0")}`,
    });
  }
  const tickWidth = (r.tick / minutesSpan) * 100;
  const tickOffset = ((first - start) / minutesSpan) * 100;
  const pick = (event: MouseEvent<HTMLDivElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    return Math.max(0, Math.min(POINTS - 1, Math.round(((event.clientX - box.left) / box.width) * (POINTS - 1))));
  };
  const busiest = useMemo(() => {
    const pool = processes.filter((p) => (p.cpu ?? 0) > 0.2 || p.mem > 200 * 1024 ** 2);
    return pool
      .map((p) => ({
        p,
        cpu: (p.cpu ?? 0) * (0.2 + noise(p.pid + at, range.length) * 0.9) * (data.cpu[at] / 20),
        mem: p.mem * (0.9 + noise(p.pid, at) * 0.2),
        io: (p.disk ?? 0) * noise(p.pid + 3, at) * 512 * 1024,
        net: (p.net ?? 0) * noise(p.pid + 5, at) * 256 * 1024,
        gpu: (p.gpu ?? 0) * noise(p.pid + 7, at) * 1.4,
      }))
      .sort((a, b) => b.cpu - a.cpu)
      .slice(0, 12);
  }, [at, range, data]);
  return (
    <div className="a-page a-history">
      <div className="a-history-card">
        <div className="a-history-rows">
          {metrics.map((m) => (
            <div className="a-history-row" key={m.key} style={{ "--c": m.color } as CSSProperties}>
              <div className="a-history-label">
                <span>{m.name}</span>
                <b>{m.value}</b>
                {m.sub && <small>{m.sub}</small>}
              </div>
              <div
                className="a-history-plot"
                role="slider"
                tabIndex={m.key === "cpu" ? 0 : -1}
                aria-label={`${m.name}，选择时刻`}
                aria-valuemin={0}
                aria-valuemax={POINTS - 1}
                aria-valuenow={at}
                aria-valuetext={moment(range, at).text}
                onPointerMove={(event) => setHover(pick(event))}
                onPointerLeave={() => setHover(null)}
                onClick={(event) => onCursor(pick(event))}
                onKeyDown={(event) => {
                  const step = event.shiftKey ? 12 : 1;
                  if (event.key === "ArrowLeft") onCursor(Math.max(0, at - step));
                  else if (event.key === "ArrowRight") onCursor(Math.min(POINTS - 1, at + step));
                  else if (event.key === "Home") onCursor(0);
                  else if (event.key === "End") onCursor(POINTS - 1);
                  else if (event.key === "Escape") onCursor(null);
                  else return;
                  event.preventDefault();
                }}
              >
                <div
                  className="a-history-ticks"
                  style={{ "--tick-w": `${tickWidth}%`, "--tick-x": `${tickOffset}%` } as CSSProperties}
                />
                <svg viewBox="0 0 1000 100" preserveAspectRatio="none" aria-hidden="true">
                  {m.lines.map((line, i) => {
                    const d = path(line.values, m.max);
                    return (
                      <g key={i}>
                        {!line.faint && <path d={d + "L1000 100L0 100Z"} fill={m.color} fillOpacity="0.16" />}
                        <path
                          d={d}
                          fill="none"
                          stroke={m.color}
                          strokeOpacity={line.faint ? 0.45 : 1}
                          strokeWidth="1.5"
                          vectorEffect="non-scaling-stroke"
                          strokeLinejoin="round"
                        />
                      </g>
                    );
                  })}
                </svg>
                {m.scale && <span className="a-history-scale">{m.scale}</span>}
                {cursor !== null && <i className="a-history-cursor" style={{ left: `${(cursor / (POINTS - 1)) * 100}%` }} />}
                {hover !== null && hover !== cursor && (
                  <i className="a-history-cursor" style={{ left: `${(hover / (POINTS - 1)) * 100}%`, opacity: 0.25 }} />
                )}
              </div>
            </div>
          ))}
        </div>
        <div className="a-history-axis" aria-hidden="true">
          {ticks.map((t) => (
            <span key={t.text + t.left} style={{ left: `${t.left}%` }}>
              {t.text}
            </span>
          ))}
        </div>
      </div>
      <div className="a-busy-card">
        <div className="a-busy-head">
          <b>{cursor === null ? "最忙的进程" : `${moment(range, cursor).text} 最忙的进程`}</b>
          <span>{cursor === null ? "单击图表上的任意时刻，查看当时的进程" : "在图表上按 ← → 前后移动，按 Esc 返回最新"}</span>
        </div>
        <div className="a-table a-busy" role="grid" aria-label="历史进程">
          <div className="a-thead" role="row">
            {[
              ["名称", ""],
              ["PID", "is-end a-c-pid"],
              ["CPU", "is-end"],
              ["内存", "is-end"],
              ["I/O", "is-end a-c-io"],
              ["网络", "is-end"],
              ["GPU", "is-end a-c-gpu"],
            ].map(([label, cls]) => (
              <span key={label} role="columnheader" className={"a-th " + cls} aria-sort={label === "CPU" ? "descending" : undefined}>
                <span className="a-th-label">
                  <span>{label}</span>
                  {label === "CPU" && <Icon name="chevronDown" size={12} />}
                </span>
              </span>
            ))}
          </div>
          <div className="a-tbody a-scroll" role="rowgroup">
            {busiest.map(({ p, cpu, mem, io, net, gpu }) => (
              <div key={p.pid} role="row" className="a-tr" onDoubleClick={() => onOpen(p.pid)} title="双击转到对应进程">
                <span role="gridcell" className="a-td">
                  <ProcessIcon glyph={p.glyph} />
                  <span className="a-cell-text">{p.exe}</span>
                </span>
                <span role="gridcell" className="a-td is-end is-faint a-c-pid">
                  {p.pid}
                </span>
                <span role="gridcell" className="a-td is-end">
                  {percent(cpu)}
                </span>
                <span role="gridcell" className="a-td is-end">
                  {bytes(mem)}
                </span>
                <span role="gridcell" className={"a-td is-end a-c-io" + (io < 1 ? " is-faint" : "")}>
                  {rate(io)}
                </span>
                <span role="gridcell" className={"a-td is-end" + (net < 1 ? " is-faint" : "")}>
                  {rate(net)}
                </span>
                <span role="gridcell" className={"a-td is-end a-c-gpu" + (gpu < 0.05 ? " is-faint" : "")}>
                  {percent(gpu)}
                </span>
              </div>
            ))}
          </div>
        </div>
        <p className="a-busy-note">
          每 5 秒记录 CPU、内存、I/O、GPU 和网络占用最高的进程（最小化或后台常驻时的刷新间隔更长时按该间隔），保留 7 天。
        </p>
      </div>
    </div>
  );
}

export function VisibilityDialog({
  rules,
  onSave,
  onClose,
}: {
  rules: VisibilityRules;
  onSave: (rules: VisibilityRules) => void;
  onClose: () => void;
}) {
  const [keep, setKeep] = useState(rules.keep.join(", "));
  const [hide, setHide] = useState(rules.hide.join(", "));
  const [invalid, setInvalid] = useState(false);
  const save = () => {
    const kept = programNames(keep),
      hidden = programNames(hide);
    if (!kept || !hidden) setInvalid(true);
    else onSave({ keep: kept, hide: hidden });
  };
  const field = (label: string, hint: string, value: string, put: (value: string) => void, placeholder?: string) => (
    <div className="a-rule">
      <span>{label}</span>
      <label className="a-field">
        <input
          value={value}
          onChange={(event) => put(event.target.value)}
          placeholder={placeholder}
          aria-label={label}
          spellCheck={false}
          autoComplete="off"
        />
      </label>
      <small>{hint}</small>
    </div>
  );
  return (
    <form
      className="a-dialog is-visibility"
      role="dialog"
      aria-modal="true"
      aria-labelledby="a-dialog-title"
      onSubmit={(event) => {
        event.preventDefault();
        save();
      }}
    >
      <div className="a-dialog-body a-scroll">
        <h3 id="a-dialog-title">编辑隐藏规则</h3>
        <p className="a-dialog-text">按 Windows 组件信息识别系统进程。两页共用以下规则，隐藏开关分别保存。</p>
        {field("始终显示", "即使属于系统进程，也保留这些程序。此名单优先于额外隐藏。", keep, setKeep)}
        {field("额外隐藏", "开启隐藏时一并隐藏这些程序。留空时仅隐藏系统进程。", hide, setHide, "额外隐藏")}
        <p className="a-rule-note">填写程序文件名，以逗号或分号分隔，不区分大小写，每项名单最多 128 个。</p>
        {invalid && (
          <p className="a-rule-error" role="alert">
            请填写有效的程序文件名，不含路径或通配符，每项名单最多 128 个。
          </p>
        )}
      </div>
      <div className="a-dialog-foot">
        <button type="submit" className="a-btn is-accent" data-primary>
          保存
        </button>
        <button
          type="button"
          className="a-btn"
          onClick={() => {
            setKeep(defaultVisibility.keep.join(", "));
            setHide("");
            setInvalid(false);
          }}
        >
          恢复默认规则
        </button>
        <button type="button" className="a-btn" onClick={onClose}>
          取消
        </button>
      </div>
    </form>
  );
}

export type ThemeChoice = "site" | "light" | "dark";
export type VitalsMode = "auto" | "show" | "hide";
/** Grouping the process list by user: automatic (several users signed in), always or never. */
export type UserGrouping = "auto" | "on" | "off";
// The accelerating proxies the updater knows (argus-app config.rs, PROXY_PRESETS).
const proxies = ["gh-proxy.com", "gh-proxy.org", "ghfast.top"];
const sections: [string, string, UiIcon][] = [
  ["language", "语言与外观", "globe"],
  ["startup", "启动与后台", "power"],
  ["hotkey", "快捷键", "keyboard"],
  ["privilege", "权限", "shield"],
  ["motion", "动画", "sparkle"],
  ["data", "数据与历史", "history"],
  ["rules", "规则", "bolt"],
  ["update", "更新", "update"],
];
function Setting({
  icon,
  title,
  desc,
  note,
  children,
}: {
  icon: UiIcon;
  title: string;
  desc?: ReactNode;
  /** A second line under the description. */
  note?: string;
  children?: ReactNode;
}) {
  return (
    <div className="a-setting">
      <Icon name={icon} />
      <div className="a-setting-text">
        <div>{title}</div>
        {desc && <span>{desc}</span>}
        {note && <span>{note}</span>}
      </div>
      {children && <div className="a-setting-controls">{children}</div>}
    </div>
  );
}
// A number box as the app's (appearance.rs): typed values round into range on
// Enter or leaving the box, the arrow keys step by one.
function NumberBox({
  label,
  value,
  range: [low, high],
  onChange,
}: {
  label: string;
  value: number;
  range: [number, number];
  onChange: (value: number) => void;
}) {
  const [text, setText] = useState<string | null>(null);
  const commit = (raw: string) => {
    const parsed = Number(raw.trim());
    onChange(raw.trim() && Number.isFinite(parsed) ? Math.max(low, Math.min(high, Math.round(parsed))) : value);
    setText(null);
  };
  return (
    <input
      className="a-number"
      aria-label={label}
      inputMode="numeric"
      value={text ?? String(value)}
      onChange={(event) => setText(event.target.value)}
      onBlur={(event) => commit(event.target.value)}
      onKeyDown={(event) => {
        if (event.key === "Enter") commit(event.currentTarget.value);
        else if (event.key === "ArrowUp" || event.key === "ArrowDown") {
          event.preventDefault();
          setText(null);
          // Held keys repeat faster than a render: step from what the box shows.
          const shown = Number(event.currentTarget.value);
          const from = Number.isFinite(shown) ? Math.round(shown) : value;
          onChange(Math.max(low, Math.min(high, from + (event.key === "ArrowUp" ? 1 : -1))));
        } else if (event.key === "Escape") setText(null);
      }}
    />
  );
}

export function Settings({
  theme,
  onTheme,
  motion,
  onMotion,
  navLeft,
  onNav,
  interval,
  onInterval,
  scale,
  onScale,
  fontSize,
  onFontSize,
  vitals,
  onVitals,
  userGroups,
  onUserGroups,
  uiMotion,
  onUiMotion,
  onVisibility,
  onSay,
}: {
  theme: ThemeChoice;
  onTheme: (t: ThemeChoice) => void;
  motion: Motion;
  onMotion: (m: Motion) => void;
  navLeft: boolean;
  onNav: (left: boolean) => void;
  interval: number;
  onInterval: (ms: number) => void;
  scale: number;
  onScale: (scale: number) => void;
  fontSize: number;
  onFontSize: (size: number) => void;
  vitals: VitalsMode;
  onVitals: (mode: VitalsMode) => void;
  userGroups: UserGrouping;
  onUserGroups: (mode: UserGrouping) => void;
  uiMotion: boolean;
  onUiMotion: (on: boolean) => void;
  onVisibility: () => void;
  onSay: (text: string) => void;
}) {
  const [values, setValues] = useState({
    autostart: false,
    tray: true,
    hotkey: true,
    windowMotion: true,
    history: true,
    autoUpdate: true,
    whatsNew: true,
    star: true,
    bits: false,
    startPage: "processes",
    trayPage: "current",
    minimized: 5000,
    background: 5000,
    release: 10,
    channel: "stable",
    route: "auto",
    proxy: proxies[0],
  });
  const [customProxy, setCustomProxy] = useState("");
  const [languageChoice, setLanguageChoice] = useState<Language | "system">(language);
  const set = <K extends keyof typeof values>(key: K, value: (typeof values)[K]) =>
    setValues((v) => ({ ...v, [key]: value }));
  const body = useRef<HTMLDivElement>(null);
  const [current, setCurrent] = useState(0);
  const picked = useRef(false);
  useEffect(() => {
    const el = body.current;
    if (!el) return;
    const follow = () => {
      if (picked.current) return;
      const top = el.getBoundingClientRect().top + 40 * scaleOf(el);
      const heads = [...el.querySelectorAll<HTMLElement>("section")];
      let index = 0;
      heads.forEach((h, i) => {
        if (h.getBoundingClientRect().top <= top) index = i;
      });
      if (el.scrollTop + el.clientHeight >= el.scrollHeight - 2) index = heads.length - 1;
      setCurrent(index);
    };
    el.addEventListener("scroll", follow, { passive: true });
    const release = () => (picked.current = false);
    el.addEventListener("wheel", release, { passive: true });
    el.addEventListener("pointerdown", release);
    return () => {
      el.removeEventListener("scroll", follow);
      el.removeEventListener("wheel", release);
      el.removeEventListener("pointerdown", release);
    };
  }, []);
  const jump = (index: number) => {
    picked.current = true;
    setCurrent(index);
    const target = body.current?.querySelectorAll("section")[index] as HTMLElement | undefined;
    if (target && body.current)
      body.current.scrollTo({
        top: target.offsetTop - 4,
        behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth",
      });
  };
  const intervals = [500, 1000, 2000, 5000].map((ms) => ({ value: ms, label: `每 ${ms / 1000} 秒` }));
  const slow = [1000, 2000, 5000, 10000, 30000].map((ms) => ({ value: ms, label: `每 ${ms / 1000} 秒` }));
  return (
    <div className="a-page a-settings a-scroll" ref={body}>
      <div className="a-settings-inner">
      <nav className="a-settings-rail" aria-label="设置分组" role="tablist">
        <span className="a-settings-mark" style={{ transform: `translateY(${current * 38}px)` }} aria-hidden="true" />
        {sections.map(([id, name, icon], i) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={i === current}
            aria-current={i === current ? "true" : undefined}
            onClick={() => jump(i)}
          >
            <Icon name={icon} />
            {name}
          </button>
        ))}
      </nav>
      <div className="a-settings-body">
        <section>
          <h2>语言与外观</h2>
          <Setting icon="globe" title="界面语言" desc="立即生效，托盘菜单与提示信息也随之切换。“跟随系统”使用 Windows 的显示语言。">
            <Select
              label="界面语言"
              value={languageChoice}
              onChange={(value) => {
                if (value !== "system" && !isLanguage(value)) return;
                setLanguageChoice(value);
                const next = value === "system" ? preferredLanguage(navigator.languages) : value;
                // The site keeps the choice, as it does one made in its header.
                rememberLanguage(next);
                if (next !== language()) location.assign(localized(next) + "#demo");
              }}
              options={[{ value: "system", label: "跟随系统" }, ...languages.map(item => ({ value: item.id, label: item.name }))]}
            />
          </Setting>
          <Setting icon="theme" title="主题" desc="“跟随系统”使用 Windows 的应用模式，系统切换浅色或深色时随之切换。">
            <Segments
              label="主题"
              value={theme}
              onChange={onTheme}
              items={[
                { value: "site", label: "跟随系统" },
                { value: "light", label: "浅色" },
                { value: "dark", label: "深色" },
              ]}
            />
          </Setting>
          <Setting
            icon={navLeft ? "navSide" : "navTop"}
            title="导航栏位置"
            desc="侧边导航栏默认仅显示图标，可通过左上角的 ☰ 按钮展开显示页面名称；顶部导航栏以标签形式排列在工具栏下方。"
          >
            <Segments
              label="导航栏位置"
              value={navLeft ? "side" : "top"}
              onChange={(v) => onNav(v === "side")}
              items={[
                { value: "side", label: "侧边", icon: "navSide" },
                { value: "top", label: "顶部", icon: "navTop" },
              ]}
            />
          </Setting>
          <Setting icon="interfaceScale" title="界面缩放" desc="整体调整文字、图标与控件，范围 80%–150%，默认 100%。立即生效并自动保存。">
            <NumberBox label="界面缩放" value={scale} range={[80, 150]} onChange={onScale} />
            <span className="a-unit">%</span>
            <Button onClick={() => onScale(100)}>恢复默认</Button>
          </Setting>
          <Setting icon="fontSize" title="字体大小" desc="基础字号为 11–20 px，默认 13 px；标题与辅助文字按比例调整，可与界面缩放组合使用。">
            <NumberBox label="字体大小" value={fontSize} range={[11, 20]} onChange={onFontSize} />
            <span className="a-unit">px</span>
            <Button onClick={() => onFontSize(13)}>恢复默认</Button>
          </Setting>
          <Setting icon="gauge" title="进程概况带" desc="自动模式预留概况带与 10 行进程的空间，空间不足时收起。折叠或筛选不影响此判断。">
            <Select
              label="进程概况带"
              context="vitals"
              value={vitals}
              onChange={onVitals}
              options={[
                { value: "auto", label: "自动" },
                { value: "show", label: "始终显示" },
                { value: "hide", label: "隐藏" },
              ]}
            />
          </Setting>
          <Setting icon="eyeOff" title="系统进程隐藏规则" desc="自定义始终显示和额外隐藏的程序。进程页与连接页共用规则，通过工具栏图标开启隐藏。">
            <Button onClick={onVisibility}>编辑隐藏规则</Button>
          </Setting>
          <Setting
            icon="users"
            title="按用户分组进程"
            desc="进程页按登录用户分组，各组内的视图、分类、排序与搜索不变。自动模式仅在多个用户同时登录（含已断开连接的会话）时分组；系统服务等无用户登录的会话归入“系统”。"
          >
            <Select
              label="按用户分组进程"
              context="userGroups"
              value={userGroups}
              onChange={onUserGroups}
              options={[
                { value: "auto", label: "自动" },
                { value: "on", label: "始终分组" },
                { value: "off", label: "不分组" },
              ]}
            />
          </Setting>
        </section>
        <section>
          <h2>启动与后台</h2>
          <Setting icon="power" title="登录 Windows 时启动" desc="以托盘方式静默启动，可通过快捷键或单击托盘图标打开窗口。">
            <Switch state label="登录 Windows 时启动" checked={values.autostart} onChange={(v) => set("autostart", v)} />
          </Setting>
          <Setting icon="processes" title="默认页面" desc="启动 Argus 后首次显示窗口时打开的页面。">
            <Select
              label="默认页面"
              value={values.startPage}
              onChange={(v) => set("startPage", v)}
              options={[
                { value: "processes", label: "进程" },
                { value: "performance", label: "性能" },
                { value: "services", label: "服务" },
                { value: "startup", label: "启动" },
                { value: "connections", label: "连接" },
                { value: "history", label: "历史" },
              ]}
            />
          </Setting>
          <Setting icon="tray" title="从托盘打开" desc="通过托盘或快捷键重新打开窗口时生效。最小化恢复仍保留当前页面。">
            <Select
              label="从托盘打开"
              value={values.trayPage}
              onChange={(v) => set("trayPage", v)}
              options={[
                { value: "current", label: "保留当前页面" },
                { value: "default", label: "打开默认页面" },
              ]}
            />
          </Setting>
          <Setting
            icon="tray"
            title="关闭窗口时保留在托盘"
            desc="保持常驻，可快速重新打开。隐藏期间按“后台常驻时的刷新间隔”采样，历史记录与规则照常运行。"
          >
            <Switch state label="关闭窗口时保留在托盘" checked={values.tray} onChange={(v) => set("tray", v)} />
          </Setting>
        </section>
        <section>
          <h2>快捷键</h2>
          <Setting
            icon="keyboard"
            title="显示或隐藏 Argus"
            desc="在大多数程序中可用。组合通常包含 Ctrl、Alt 或 Win 之一和一个普通键，修饰键不区分左右。"
          >
            <span className="a-keys">
              <kbd>Ctrl</kbd>
              <kbd>Alt</kbd>
              <kbd>Esc</kbd>
            </span>
            <Button>更改</Button>
            <Switch state label="显示或隐藏 Argus" checked={values.hotkey} onChange={(v) => set("hotkey", v)} />
          </Setting>
          <Setting icon="keyboard" title="快捷键说明" desc="查看页面切换、刷新暂停、进程操作等快捷键。">
            <Button onClick={() => onSay("Ctrl+1–8 切换页面；Ctrl+F 搜索进程；Delete 结束选中的进程；按住 Ctrl 暂停刷新。")}>
              查看快捷键
            </Button>
          </Setting>
        </section>
        <section>
          <h2>权限</h2>
          <Setting icon="shield" title="管理员运行" desc="授权一次后，登录启动、快捷键及任务管理器入口将以管理员身份运行。">
            <span className="a-pill is-ok">已授权</span>
          </Setting>
          <Setting
            icon="shield"
            title="特权服务"
            desc="使以标准权限运行的 Argus 能够管理系统进程、自动应用规则、采集每进程网络流量与磁盘读写、查询文件占用与磁盘 SMART 信息，以及控制服务和断开 IPv4 TCP 连接。安装和卸载均需要管理员授权。"
          >
            <span className="a-pill is-ok">{`已连接 · ${appVersion}`}</span>
          </Setting>
          <Setting
            icon="bolt"
            title="内核传感器"
            desc="开启时自动检查 PawnIO：缺少或版本过旧时安装官方驱动，已安装时启动驱动；需要管理员授权。需先安装并启动 Argus 特权服务。关闭仅停止 Argus 的采集。"
          >
            <Button icon="external">PawnIO 官网</Button>
            <Switch state label="内核传感器" checked onChange={() => onSay("演示窗口不会停止内核传感器的采集。")} />
          </Setting>
          <Setting
            icon="keyboard"
            title="替换系统任务管理器"
            desc="使 Ctrl+Shift+Esc 等入口打开 Argus。默认关闭；独立恢复程序不依赖界面或服务。更新特权服务时同步更新替换程序并保持开启；卸载服务时恢复 Windows 任务管理器。"
          >
            <span className="a-pill">使用 Windows 任务管理器</span>
            <Button
              icon="external"
              title="仅本次打开 Windows 任务管理器，保留当前替换设置。"
              onClick={() => onSay("演示窗口不会打开 Windows 任务管理器。")}
            >
              临时打开系统任务管理器
            </Button>
          </Setting>
        </section>
        <section>
          <h2>动画</h2>
          <Setting icon="genericApp" title="窗口过渡动画" desc="窗口显示、隐藏、最小化与还原时的淡入淡出。关闭后由系统或其他窗口管理软件处理窗口过渡。默认跟随 Windows 的“动画效果”设置。">
            <Switch state label="窗口过渡动画" checked={values.windowMotion} onChange={(v) => set("windowMotion", v)} />
          </Setting>
          <Setting icon="sparkle" title="界面动画" desc="页面和弹窗的过渡、滑动的选中标记，以及图标的动效。默认跟随 Windows 的“动画效果”设置。">
            <Switch state label="界面动画" checked={uiMotion} onChange={onUiMotion} />
          </Setting>
          <Setting
            icon="pulse"
            title="曲线动画"
            desc="帧率越高，曲线越流畅，资源占用也越高：流畅模式约占单核 10%–15%，均衡模式约为其一半；省电模式每次移动一个整像素，较小图表的移动不连续。"
          >
            <Select
              label="曲线动画"
              value={motion}
              onChange={onMotion}
              options={[
                { value: "smooth", label: "流畅（跟随刷新率）" },
                { value: "step", label: "关闭（每次采样跳一格）" },
              ]}
            />
          </Setting>
        </section>
        <section>
          <h2>数据与历史</h2>
          <Setting icon="timer" title="刷新频率" desc="间隔越短，曲线越精细，Argus 自身的资源开销也越高。">
            <Select label="刷新频率" value={interval} onChange={onInterval} options={intervals} />
          </Setting>
          <Setting
            icon="minimize"
            title="最小化时的刷新间隔"
            desc="窗口最小化后按此间隔采样。间隔越长，后台开销越低，但规则对新启动程序的生效最多延后一个间隔；长于 5 秒时，历史记录也随之变稀。"
          >
            <Select label="最小化时的刷新间隔" value={values.minimized} onChange={(v) => set("minimized", v)} options={slow} />
          </Setting>
          <Setting
            icon="tray"
            title="后台常驻时的刷新间隔"
            desc="隐藏到托盘后按此间隔采样，托盘的悬停提示随之更新。间隔越长，后台开销越低，但规则对新启动程序的生效最多延后一个间隔；长于 5 秒时，历史记录也随之变稀。"
          >
            <Select label="后台常驻时的刷新间隔" value={values.background} onChange={(v) => set("background", v)} options={slow} />
          </Setting>
          <Setting icon="memory" title="隐藏时释放内存" desc="窗口最小化或隐藏到托盘达到设定时长后，Argus 释放图表图像，并将未使用的内存归还 Windows。采样和历史记录照常进行。再次打开窗口时会先重绘图表，可能稍有停顿。">
            <Select
              label="隐藏时释放内存"
              value={values.release}
              onChange={(v) => set("release", v)}
              options={[0, 1, 10, 30].map((minutes) => ({
                value: minutes,
                label: minutes ? `${minutes} 分钟后` : "从不",
              }))}
            />
          </Setting>
          <Setting
            icon="transfer"
            title="网络速率单位"
            desc="字节与磁盘速率的单位一致，比特与网卡链路速率、宽带带宽的单位一致。性能页的网络视图同时显示两种单位。"
          >
            <Segments
              label="网络速率单位"
              value={values.bits ? "bits" : "bytes"}
              onChange={(v) => set("bits", v === "bits")}
              items={[
                { value: "bytes", label: "MB/s", title: "字节每秒" },
                { value: "bits", label: "Mbps", title: "比特每秒" },
              ]}
            />
          </Setting>
          <Setting
            icon="history"
            title="记录历史"
            desc={
              values.history
                ? "每 5 秒记录一次资源占用及占用最高的进程；最小化或后台常驻时的刷新间隔长于 5 秒时，按该间隔记录。数据仅保存在本机，保留 7 天，可在“历史”页查看。"
                : "已关闭。现有记录将在 7 天后过期，也可立即清除。"
            }
          >
            <Switch state label="记录历史" checked={values.history} onChange={(v) => set("history", v)} />
          </Setting>
          <Setting icon="storage" title="已保存的历史" desc="占用 38.6 MB，最早记录于 10/27 09:12。">
            <Button icon="trash" onClick={() => onSay("演示窗口不会清除历史记录。")}>
              清除
            </Button>
          </Setting>
        </section>
        <section>
          <h2>规则</h2>
          <Setting
            icon="bolt"
            title="程序启动时自动设置优先级或效率模式"
            desc="保存后对正在运行的进程立即生效。也可在进程列表中右键单击进程，选择“添加规则”。"
          >
            <input className="a-number" style={{ width: 190 }} placeholder="程序名，例如 chrome.exe" aria-label="新规则的程序名" />
            <Button onClick={() => onSay("演示窗口不会保存规则。")}>添加</Button>
          </Setting>
        </section>
        <section>
          <h2>更新</h2>
          <Setting icon="update" title="已是最新版本" desc={`当前版本 ${appVersion} · 稳定版`} note="更新来自 GitHub Releases 或 Argus 官方镜像，安装前校验安装包签名。">
            <Button onClick={() => onSay("已是最新版本。")}>检查更新</Button>
          </Setting>
          <Setting icon="timer" title="自动检查更新" desc="启动约 1 分钟后检查一次，此后每天检查一次。新版本在后台下载，安装需手动确认。">
            <Switch state label="自动检查更新" checked={values.autoUpdate} onChange={(v) => set("autoUpdate", v)} />
          </Setting>
          <Setting icon="notes" title="更新后显示更新内容" desc="更新完成后首次打开窗口时显示新版本的更新内容，每个版本仅显示一次。关闭此项后，仅在右下角提示已更新。">
            <Button onClick={() => onSay("演示窗口不显示更新内容，可在官网的“更新日志”页查看。")}>查看本版本更新</Button>
            <Switch state label="更新后显示更新内容" checked={values.whatsNew} onChange={(v) => set("whatsNew", v)} />
          </Setting>
          <Setting icon="sparkle" title="更新通道" desc="测试版提前提供新功能，稳定性可能不及稳定版。由测试版切换到稳定版后，需等到稳定版的版本号高于当前版本时才会更新。">
            <Select
              label="更新通道"
              value={values.channel}
              onChange={(v) => set("channel", v)}
              options={[
                { value: "stable", label: "稳定版" },
                { value: "beta", label: "测试版" },
              ]}
            />
          </Setting>
          <Setting icon="globe" title="下载线路" desc="加速代理由第三方提供，可获知本机的 IP 地址及所下载的文件。安装包签名同时覆盖版本号，代理无法篡改更新，也无法以旧版本冒充新版本。">
            <Select
              label="下载线路"
              value={values.route}
              onChange={(v) => set("route", v)}
              options={[
                { value: "auto", label: "自动（GitHub、官方镜像、加速代理）" },
                { value: "direct", label: "仅连接 GitHub" },
                { value: "proxy", label: "仅使用加速代理" },
              ]}
            />
          </Setting>
          {values.route !== "direct" && (
            <Setting
              icon="transfer"
              title="加速代理"
              desc={values.route === "auto" ? "GitHub 和官方镜像均连接失败时，依次尝试此代理和其他内置代理。" : "所有更新请求均经由此代理。"}
            >
              <Select
                label="加速代理"
                value={values.proxy}
                onChange={(v) => set("proxy", v)}
                options={[...proxies.map((host) => ({ value: host, label: host })), { value: "custom", label: "自定义…" }]}
              />
            </Setting>
          )}
          {values.route !== "direct" && values.proxy === "custom" && (
            <Setting icon="globe" title="自定义代理" desc="添加在 GitHub 地址之前的前缀，须使用 https，例如 https://example.com/。">
              <input
                className="a-number"
                style={{ width: 220 }}
                value={customProxy}
                onChange={(event) => setCustomProxy(event.target.value)}
                placeholder="https://"
                aria-label="自定义代理地址"
                spellCheck={false}
                autoComplete="off"
              />
              <Button disabled={!customProxy.trim()}>保存</Button>
            </Setting>
          )}
          <Setting icon="star" title="Star 提醒" desc="每天在窗口右下角提醒一次，邀请在 GitHub 上为 Argus 点亮 Star。从提醒前往 GitHub 或选择“不再提醒”后，此项自动关闭。">
            <Switch state label="Star 提醒" checked={values.star} onChange={(v) => set("star", v)} />
          </Setting>
        </section>
      </div>
      </div>
    </div>
  );
}

// A link card's moving parts (about.rs `project_link`): the hover face, the
// line along its top edge, and its arrow, muted and in the link's colour.
function LinkCardParts() {
  return (
    <>
      <span className="a-link-face" aria-hidden="true" />
      <span className="a-link-line" aria-hidden="true" />
      <span className="a-link-arrow" aria-hidden="true">
        <Icon name="external" size={15} />
        <Icon name="external" size={15} className="is-lit" />
      </span>
    </>
  );
}

// About (about.rs): the app and this build, then where to find help, in one
// column as wide as the settings' content.
export function About({
  onCopy,
  onQuit,
  onSay,
}: {
  onCopy: (text: string, what: string) => void;
  onQuit: () => void;
  onSay: (text: string) => void;
}) {
  return (
    <div className="a-page a-about a-scroll">
      <div className="a-about-inner">
        <div className="a-about-hero">
          <img className="a-about-logo" src="/assets/argus.svg" width="64" height="64" alt="" />
          <div className="a-about-text">
            <div className="a-about-name">
              <strong>Argus</strong>
              <small>{appVersion}</small>
            </div>
            <p>快速、现代、信息丰富的 Windows 任务管理器。</p>
            <span>正在以管理员身份运行。</span>
            <span>© 2026 Argus contributors</span>
          </div>
          <div className="a-setting-controls">
            <Button onClick={() => onSay("演示窗口不显示更新内容，可在官网的“更新日志”页查看。")}>更新内容</Button>
            <Button onClick={onQuit}>退出 Argus</Button>
          </div>
        </div>
        <div className="a-about-links">
          <a className="a-link-card" href="https://argustask.com/" target="_blank" rel="noopener">
            <LinkCardParts />
            <img className="a-link-mark" src="/assets/argus.svg" width="44" height="44" alt="" />
            <strong>Argus 官网</strong>
            <span className="a-link-address">argustask.com</span>
            <span>下载稳定版，阅读使用文档、常见问题和更新日志。</span>
          </a>
          <a className="a-link-card is-repo" href="https://github.com/Ray-lea/Argus" target="_blank" rel="noopener">
            <LinkCardParts />
            <span className="a-link-mark">
              <Icon name="github" size={26} />
            </span>
            <strong>GitHub 仓库</strong>
            <span className="a-link-address">github.com/Ray-lea/Argus</span>
            <span>查看源代码，报告问题或提出建议，获取各版本的安装包。</span>
          </a>
        </div>
        <h2>项目与支持</h2>
        <Setting icon="chat" title="交流与反馈" desc="加入 QQ 群 453803497，反馈问题、提出建议或交流使用心得。">
          <Button onClick={() => onCopy("453803497", "群号")}>复制群号</Button>
          <Button tone="accent">加入 QQ 群</Button>
        </Setting>
        <Setting icon="channel" title="Telegram 频道" desc="加入 Telegram 频道 t.me/argustask，获取版本更新与项目动态。">
          <Button onClick={() => onCopy("https://t.me/argustask", "频道链接")}>复制链接</Button>
          <Button tone="accent">加入频道</Button>
        </Setting>
        <Setting icon="sparkle" title="首次启动引导" desc="重新查看常用设置与使用提示。">
          <Button>打开引导</Button>
        </Setting>
        <Setting
          icon="document"
          title="许可证"
          desc="Argus 是自由软件，依据 GNU Affero General Public License 第 3 版或任何更高版本（AGPL-3.0-or-later）发布，可按该许可证的条款再分发和修改。Argus 不提供任何担保。"
        >
          <Button>查看许可证</Button>
        </Setting>
        <Setting icon="document" title="第三方组件" desc="Argus 包含的开源组件及其许可证与版权声明。">
          <Button>查看许可声明</Button>
        </Setting>
        <Setting
          icon="storage"
          title="日志"
          desc="记录运行环境、功能状态、失败步骤、错误码及相关文件或目录的实际路径。仅保存在本机，每个文件最多 1 MB，并保留最近 3 份旧日志；不记录命令行、凭据或文件内容。缺陷报告请使用此入口打开的诊断日志。"
        >
          <Button onClick={() => onSay("演示窗口不会打开日志文件夹。")}>打开日志文件夹</Button>
        </Setting>
      </div>
    </div>
  );
}
