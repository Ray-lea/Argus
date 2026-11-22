/** @jsxImportSource @/components/replica/i18n */
"use client";
import {
  useEffect,
  useMemo,
  useState,
  type CSSProperties,
  type MouseEvent,
  type PointerEvent,
  type ReactNode,
} from "react";
import { Icon, ProcessIcon, type UiIcon } from "./icons";
import Welcome from "./Welcome";
import SectionPage from "./SectionPage";
import Hundred from "./Hundred";
import HistorySettings from "./HistorySettings";
import { store, useShared } from "./store";
import { machine } from "./model";
import { copyEntries } from "./tableActions";
import type { Motion } from "./Curve";
import { language, translate } from "./i18n";
import { dateLocale, isLanguage, languages, localized, preferredLanguage, rememberLanguage, type Language } from "@/lib/languages";
import {
  appVersion,
  connections,
  defaultVisibility,
  processes,
  programNames,
  startLabel,
  stateLabel,
  type Process,
  type Service,
  type StartupItem,
  type VisibilityRules,
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
  filter?: ReactNode;
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
  const edge = (event: PointerEvent<HTMLElement>, key: string) => {
    const grid = event.currentTarget.closest<HTMLElement>(".a-table")!;
    const col = event.currentTarget.parentElement!;
    const grip = event.currentTarget;
    const origin = event.clientX,
      width = col.offsetWidth,
      k = scaleOf(col);
    grip.setPointerCapture(event.pointerId);
    const move=(e:globalThis.PointerEvent)=>grid.style.setProperty("--col-"+key, Math.min(grid.clientWidth*.55,Math.max(48,width+(e.clientX-origin)/k))+"px");
    const end=()=>{grip.removeEventListener("pointermove",move);grip.removeEventListener("lostpointercapture",end)};
    grip.addEventListener("pointermove",move);grip.addEventListener("lostpointercapture",end);
  };
  return (
    <div className="a-thead" role="row">
      {cols.map((c) => (
        <div key={c.key} role="columnheader" aria-sort={sort.key === c.key ? (sort.desc ? "descending" : "ascending") : "none"} className={"a-th a-header-controls" + (c.end ? " is-end" : "") + (c.className ? " " + c.className : "")}>
        <button
          type="button"
          className="a-sort-button"
          onClick={() => onSort(c.key)}
        >
          <span className="a-th-label">
            <span>{c.label}</span>
            {sort.key === c.key && <Icon name={sort.desc ? "chevronDown" : "chevronUp"} size={12} />}
          </span>
        </button>{c.filter}{!["description","command"].includes(c.key)&&<i className="a-list-edge" aria-hidden="true"
          onPointerDown={e=>edge(e,c.key)} onDoubleClick={e=>e.currentTarget.closest<HTMLElement>(".a-table")?.style.removeProperty("--col-"+c.key)}/>}</div>
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

// Measure the full collection, so a filter or search leaves the columns in place.
function measuredColumns(fields:Record<string,[string[],number,number]>) {
  if(typeof document==="undefined")return undefined;
  const canvas=document.createElement("canvas").getContext("2d");
  if(!canvas)return undefined;
  canvas.font='13px "Segoe UI Variable Text","Segoe UI","Microsoft YaHei UI",sans-serif';
  return Object.fromEntries(Object.entries(fields).map(([id,[texts,padding,min]])=>{
    const widths=texts.map(text=>canvas.measureText(translate(text)).width).sort((a,b)=>a-b);
    return ["--measure-"+id, Math.ceil(Math.max(min,(widths[Math.floor((widths.length-1)*.95)]??0)+padding))+"px"];
  })) as CSSProperties;
}
type ServiceCol = "display" | "name" | "pid" | "state" | "start" | "description";
export function Services({
  list,
  states,
  filter,
  onFilter,
  query,
  selected,
  onSelect,
  onMenu,
}: {
  list: Service[];
  states: Map<string, ServiceState>;
  filter: ServiceFilter;
  onFilter: (value: ServiceFilter) => void;
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
  const widths=useMemo(()=>measuredColumns({display:[list.map(s=>s.display),24,160],name:[list.map(s=>s.name),22,96],state:[Object.values(stateText),36,84],start:[Object.values(startLabel),24,100]}),[list]);
  const running = list.filter((s) => states.get(s.name) === "running").length;
  return (
    <div className="a-page">
      <div className="a-list-card">
        <div className="a-table a-services" style={widths} role="grid" aria-label="服务">
          <Header
            sort={sort}
            onSort={setSort}
            cols={[
              { key: "display", label: "服务" },
              { key: "name", label: "名称", className: "a-c-name" },
              { key: "pid", label: "PID", end: true, className: "a-c-pid" },
              {
                key: "state",
                label: "状态",
                filter: (
                  <Select
                    className="a-filter-select"
                    label="筛选"
                    value={filter}
                    onChange={onFilter}
                    options={[
                      { value: "all", label: "全部" },
                      { value: "running", label: "正在运行" },
                      { value: "stopped", label: "已停止" },
                    ]}
                  />
                ),
              },
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
  onDelete,
  onMenu,
}: {
  items: StartupItem[];
  query: string;
  onToggle: (name: string) => void;
  onDelete: (item: StartupItem) => void;
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
  const widths=useMemo(()=>measuredColumns({name:[items.map(i=>i.name),items.some(i=>i.missing)?80:48,144],publisher:[items.map(i=>i.publisher),24,132],location:[items.map(i=>i.location),24,138]}),[items]);
  const enabled = items.filter((i) => i.enabled).length;
  return (
    <div className="a-page">
      <div className="a-list-card">
        <div className="a-table a-startup" style={widths} role="grid" aria-label="启动项">
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
                    ...copyEntries(event.target as Element,item.name,[{ label: "复制命令", run: () => void navigator.clipboard?.writeText(item.command) }]),
                    {label:"删除", icon:"trash",danger:true,disabled:item.location.includes("应用包"),run:()=>onDelete(item)},
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
          <span>禁用后可重新启用。更改“所有用户”的启动项需要管理员权限。</span>
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
  onFilter,
  grouped,
  query,
  hidden,
  onShowAll,
  onOpen,
  onMenu,
}: {
  alive: Process[];
  filter: ConnectionFilter;
  onFilter: (value: ConnectionFilter) => void;
  grouped: boolean;
  query: string;
  /** While system processes are hidden: whether a process is one of them. */
  hidden?: (proc: Process) => boolean;
  onShowAll: () => void;
  onOpen: (pid: number) => void;
  onMenu: OpenMenu;
}) {
  const [sort, setSort] = useSort<ConnectionCol>("name");
  const [protocol, setProtocol] = useState("all");
  const [opened, setOpened] = useState<Set<string>>(new Set());
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
      if (protocol !== "all" && c.protocol + (c.local.includes(":") ? "v6" : "") !== protocol) return false;
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
  type Entry = (typeof rows)[number];
  const groups = new Map<string, Entry[]>();
  const shown: { entry: Entry; group?: string; count?: number; child?: boolean; pid?: number }[] = [];
  if (grouped) {
    for (const entry of rows) {
      const key = entry.pid === 0 ? "|unowned" : !living.has(entry.pid) || !entry.proc ? "|exited"
        : (entry.proc.path || entry.proc.exe).toLowerCase();
      const group = groups.get(key);
      if (group) group.push(entry);
      else groups.set(key, [entry]);
    }
    for (const [group, members] of groups) {
      const first = members[0];
      if (members.length === 1) {
        shown.push({ entry: first });
        continue;
      }
      const pid = members.every((entry) => entry.pid === first.pid) ? first.pid : undefined;
      shown.push({ entry: first, group, count: members.length, pid });
      if (q || opened.has(group)) shown.push(...members.map((entry) => ({ entry, child: true })));
    }
  } else {
    for (const entry of rows) shown.push({ entry });
  }
  const toggle = (group: string) => {
    if (q) return;
    setOpened((current) => {
      const next = new Set(current);
      if (next.has(group)) next.delete(group);
      else next.add(group);
      return next;
    });
  };
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
              { key: "protocol", label: "协议",filter:<Select className="a-filter-select" label="协议" value={protocol} onChange={setProtocol} options={["all","TCP","TCPv6","UDP","UDPv6"].map(value=>({value,label:value==="all"?"全部":value}))}/> },
              { key: "local", label: "本地地址", className: "a-c-local" },
              { key: "localPort", label: "本地端口", end: true },
              { key: "remote", label: "远程地址", className: "a-c-remote" },
              { key: "remotePort", label: "远程端口", end: true, className: "a-c-rport" },
              { key: "state", label: "状态",filter:<Select className="a-filter-select" label="状态" value={filter} onChange={onFilter} options={[{value:"all",label:"全部"},{value:"listen",label:"监听"},{value:"established",label:"已连接"},{value:"udp",label:"UDP"}]}/> },
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
            {shown.map(({ entry: c, group, count, child, pid }) => {
              const gone = !living.has(c.pid);
              if (group !== undefined) {
                const expanded = Boolean(q) || opened.has(group);
                const name = group === "|unowned" ? "无所属进程" : group === "|exited" ? "已退出" : c.proc?.name;
                return (
                  <div key={group} role="row" className="a-tr" onDoubleClick={() => toggle(group)}
                    onContextMenu={(event) => {
                      event.preventDefault();
                      onMenu(event, [
                        { label: expanded ? "折叠" : "展开", disabled: Boolean(q), run: () => toggle(group) },
                        ...copyEntries(event.target as Element, name ?? String(c.pid)),
                        { label: "转到进程", icon: "goTo", disabled: gone || !pid, run: () => pid && onOpen(pid) },
                      ]);
                    }}>
                    <span role="gridcell" className="a-td a-td-name" title={gone ? undefined : c.proc?.path}>
                      <button type="button" className="a-fold" aria-expanded={expanded} aria-label={expanded ? "折叠" : "展开"}
                        disabled={Boolean(q)} onClick={() => toggle(group)} onDoubleClick={(event) => event.stopPropagation()}>
                        <Icon name="chevronRight" size={12} />
                      </button>
                      <ProcessIcon glyph={gone ? "app" : c.proc?.glyph ?? "app"} />
                      <span className="a-name is-strong">{name}</span>
                      <span className="a-count">{count}</span>
                    </span>
                    <span role="gridcell" className="a-td is-end is-faint a-c-pid">{pid || ""}</span>
                  </div>
                );
              }
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
                      ...copyEntries(event.target as Element,c.proc?.exe ?? String(c.pid), [{ label: "复制本地地址", icon: "copy", run: () => void navigator.clipboard?.writeText(endpoint(c.local, c.localPort)).catch(() => {}) },
                      { label: "复制远程地址", disabled: !c.remote, run: () => void navigator.clipboard?.writeText(endpoint(c.remote ?? "", c.remotePort)).catch(() => {}) }]),
                      "-",
                      { label: "置顶", icon: "pin" },
                      { label: "添加备注", icon: "note" },
                    ]);
                  }}
                >
                  <span role="gridcell" className="a-td" title={c.proc?.path} style={grouped ? { paddingLeft: child ? 26 : 6 } : undefined}>
                    {grouped && <span className="a-fold-gap" />}
                    <ProcessIcon glyph={c.proc?.glyph ?? "app"} />
                    <span className="a-cell-text">{c.proc?.name ?? "PID " + c.pid}</span>
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
          <span>双击连接可转到对应进程。</span>
        </footer>
      </div>
    </div>
  );
}

export type { HistoryRange } from "./model";

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
        <p className="a-dialog-text">进程页和连接页共用以下规则，分别保存隐藏开关。</p>
        {field("始终显示", "优先于系统进程和“额外隐藏”规则。", keep, setKeep)}
        {field("额外隐藏", "仅在开启“隐藏系统进程”时生效。", hide, setHide, "额外隐藏")}
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
  onShortcuts,
  pauseSorting,
  onPauseSorting,
  section,
  onComponents,
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
  onShortcuts: () => void;
  onUiMotion: (on: boolean) => void;
  pauseSorting: boolean;
  onPauseSorting: (on: boolean) => void;
  section: string;
  onComponents: () => void;
  onVisibility: () => void;
  onSay: (text: string) => void;
}) {
  const { recording, leakHints, ignoreLeak } = useShared();
  const [values, setValues] = useState({
    autostart: false,
    tray: true,
    hotkey: true,
    windowMotion: true,
    hardwareAcceleration: true,
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
  const [refreshHz,setRefreshHz]=useState(60);
  useEffect(()=>{let frame=0,previous=0;const periods:number[]=[];
    const measure=(time:number)=>{if(previous)periods.push(time-previous);previous=time;if(periods.length<16)frame=requestAnimationFrame(measure);else{periods.sort((a,b)=>a-b);setRefreshHz(Math.max(1,Math.round(1000/periods[8])))}};frame=requestAnimationFrame(measure);return()=>cancelAnimationFrame(frame);
  },[]);
  const [customProxy, setCustomProxy] = useState("");
  const [languageChoice, setLanguageChoice] = useState<Language | "system">(language);
  const set = <K extends keyof typeof values>(key: K, value: (typeof values)[K]) =>
    setValues((v) => ({ ...v, [key]: value }));
  const intervals = [500, 1000, 2000, 5000].map((ms) => ({ value: ms, label: `每 ${ms / 1000} 秒` }));
  const slow = [1000, 2000, 5000, 10000, 30000].map((ms) => ({ value: ms, label: `每 ${ms / 1000} 秒` }));
  return (
    <SectionPage title="设置分组" entries={sections} target={section}>
        <section>
          <h2>语言与外观</h2>
          <Setting icon="globe" title="界面语言">
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
          <Setting icon="theme" title="主题">
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
            desc="侧边导航栏默认仅显示图标，可单击左上角的 ☰ 展开。"
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
          <Setting icon="interfaceScale" title="界面缩放" desc="整体调整文字、图标与控件，范围 80%–150%。">
            <NumberBox label="界面缩放" value={scale} range={[80, 150]} onChange={onScale} />
            <span className="a-unit">%</span>
            <Button onClick={() => onScale(100)}>恢复默认</Button>
          </Setting>
          <Setting icon="fontSize" title="字体大小" desc="基础字号 11–20 px，标题与辅助文字按比例调整。">
            <NumberBox label="字体大小" value={fontSize} range={[11, 20]} onChange={onFontSize} />
            <span className="a-unit">px</span>
            <Button onClick={() => onFontSize(13)}>恢复默认</Button>
          </Setting>
          <Setting icon="gauge" title="进程概况带" desc="自动模式在窗口空间不足时收起概况带。">
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
            desc="自动模式仅在多个用户同时登录时分组，包括已断开连接的会话。"
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
          <Setting icon="order" title="指针悬停时暂停排序"><Switch state label="指针悬停时暂停排序" checked={pauseSorting} onChange={onPauseSorting}/></Setting>
          <Setting icon="warning" title="资源泄漏提示" desc="在进程页标出可能泄漏句柄或进程的程序。">{ignoreLeak&&<Button onClick={()=>store.set({ignoreLeak:false})}>恢复 1 个已忽略的程序</Button>}<Switch state label="资源泄漏提示" checked={leakHints} onChange={v=>store.set({leakHints:v})}/></Setting>
        </section>
        <section>
          <h2>启动与后台</h2>
          <Setting icon="power" title="登录 Windows 时启动" desc="在托盘中静默启动。">
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
          <Setting icon="tray" title="托盘与组件" desc="包括托盘图标、快捷面板、关闭窗口时的行为与后台常驻时的刷新间隔。"><Button onClick={onComponents}>打开组件页</Button></Setting>
        </section>
        <section>
          <h2>快捷键</h2>
          <Setting
            icon="keyboard"
            title="显示或隐藏 Argus"
            desc="在大多数程序中可用。"
          >
            <span className="a-keys">
              <kbd>Ctrl</kbd>
              <kbd>Alt</kbd>
              <kbd>Esc</kbd>
            </span>
            <Button>更改</Button>
            <Switch state label="显示或隐藏 Argus" checked={values.hotkey} onChange={(v) => set("hotkey", v)} />
          </Setting>
          <Setting icon="keyboard" title="快捷键说明">
            <Button onClick={onShortcuts}>
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
            desc="使以标准权限运行的 Argus 能够管理系统进程与服务，并采集需要管理员权限的数据。"
          >
            <span className="a-pill is-ok">{`已连接 · ${appVersion}`}</span>
          </Setting>
          <Setting
            icon="bolt"
            title="内核传感器"
            desc="需先启用特权服务。安装、更新或启动 PawnIO 驱动需要管理员授权。关闭后停止 Argus 的内核采集，保留驱动。"
          >
            <Button icon="external">PawnIO 官网</Button>
            <Switch state label="内核传感器" checked onChange={() => onSay("演示窗口不会停止内核传感器的采集。")} />
          </Setting>
          <Setting
            icon="keyboard"
            title="替换系统任务管理器"
            desc="使 Ctrl+Shift+Esc 等入口打开 Argus。"
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
          <Setting icon="genericApp" title="窗口过渡动画" desc="窗口显示、隐藏、最小化与还原时的淡入淡出。默认跟随 Windows 的“动画效果”设置。">
            <Switch state label="窗口过渡动画" checked={values.windowMotion} onChange={(v) => set("windowMotion", v)} />
          </Setting>
          <Setting icon="sparkle" title="界面动画" desc="页面和弹窗的过渡、滑动的选中标记，以及图标的动效。默认跟随 Windows 的“动画效果”设置。">
            <Switch state label="界面动画" checked={uiMotion} onChange={onUiMotion} />
          </Setting>
          <Setting
            icon="pulse"
            title="曲线动画"
            desc="帧率越高，曲线越流畅，资源占用也越高。"
          >
            <Select
              label="曲线动画"
              value={motion}
              onChange={onMotion}
              options={[
                {value:"smooth",label:`流畅（${refreshHz} 帧/秒）`},
                {value:"balanced",label:`均衡（${Math.round(refreshHz/Math.max(1,Math.floor(refreshHz/60)))} 帧/秒）`},
                {value:"pixel",label:"省电（逐像素移动）"},
                { value: "step", label: "关闭（每次采样跳一格）" },
              ]}
            />
          </Setting>
          <Setting
            icon="gpu"
            title="硬件加速"
            desc="关闭后改由 CPU 绘制界面，不再占用 GPU，CPU 占用随之升高。"
          >
            <Switch
              state
              label="硬件加速"
              checked={values.hardwareAcceleration}
              onChange={(v) => set("hardwareAcceleration", v)}
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
            desc="间隔越长，后台开销越低，但规则对新启动程序的生效最多延后一个间隔；长于 5 秒时，历史记录也随之变稀。"
          >
            <Select label="最小化时的刷新间隔" value={values.minimized} onChange={(v) => set("minimized", v)} options={slow} />
          </Setting>
          <Setting icon="memory" title="隐藏时释放内存" desc="窗口最小化或隐藏到托盘达到设定时长后，释放图表图像等占用的内存，再次打开窗口时可能稍有停顿。">
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
            desc="字节与磁盘速率一致，比特与网卡链路速率、宽带带宽一致。"
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
              recording.enabled
                ? "通常每 5 秒记录一次资源占用及占用最高的进程，数据仅保存在本机。"
                : "已关闭。现有记录仍按保留上限删除，也可立即清除。"
            }
          >
            <Switch state label="记录历史" checked={recording.enabled} onChange={(v) => store.set({recording:{...recording,enabled:v,until:v?null:Math.floor(performance.now()/1000)}})} />
          </Setting>
          <HistorySettings/>
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
          <Setting icon="timer" title="自动检查更新" desc="每天检查一次，新版本在后台下载，安装前需手动确认。">
            <Switch state label="自动检查更新" checked={values.autoUpdate} onChange={(v) => set("autoUpdate", v)} />
          </Setting>
          <Setting icon="notes" title="更新后显示更新内容" desc="关闭后仅在右下角提示已更新。">
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
          <Setting icon="globe" title="下载线路" desc="加速代理由第三方提供，可获知本机的 IP 地址及所下载的文件；安装包须通过签名与版本号校验。">
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
          <Setting icon="star" title="Star 提醒" desc="每天在窗口右下角提醒一次，邀请在 GitHub 上为 Argus 点亮 Star。">
            <Switch state label="Star 提醒" checked={values.star} onChange={(v) => set("star", v)} />
          </Setting>
        </section>
    </SectionPage>
  );
}

// A link card's moving parts (about.rs `project_link`): the hover face, the
// line along its top edge, and its arrow, muted and in the link's colour.
// A link card (about.rs `project_link`): the mark beside the name and address,
// the whole card opening the address, and along its foot the pages there.
function LinkCard({
  repo,
  href,
  mark,
  title,
  address,
  pages,
}: {
  repo?: boolean;
  href: string;
  mark: ReactNode;
  title: string;
  address: string;
  pages: [UiIcon, string, string][];
}) {
  return (
    <div className={"a-link-card" + (repo ? " is-repo" : "")}>
      <span className="a-link-face" aria-hidden="true" />
      <span className="a-link-line" aria-hidden="true" />
      <a className="a-link-head" href={href} target="_blank" rel="noopener">
        {mark}
        <span className="a-link-name">
          <strong>{title}</strong>
          <span className="a-link-address">{address}</span>
        </span>
        <span className="a-link-arrow" aria-hidden="true">
          <Icon name="external" size={14} />
          <Icon name="external" size={14} className="is-lit" />
        </span>
      </a>
      <nav className="a-link-pages">
        {pages.map(([icon, text, url]) => (
          <a key={url} className="a-btn is-ghost is-small" href={url} target="_blank" rel="noopener">
            <Icon name={icon} size={14} />
            {text}
          </a>
        ))}
      </nav>
    </div>
  );
}

// About (about.rs): the app and this build, then where to find help, in one
// column as wide as the settings' content.
export function About({
  tick,
  interval,
  navLeft,
  onNav,
  onInterval,
  onCopy,
  onQuit,
  onSay,
}: {
  onCopy: (text: string, what: string) => void;
  tick: number;
  interval: number;
  navLeft: boolean;
  onNav: (v: boolean) => void;
  onInterval: (ms: number) => void;
  onQuit: () => void;
  onSay: (text: string) => void;
}) {
  const [welcome, setWelcome] = useState(false);
  return (
    <div className="a-page a-about a-scroll">
      {welcome && (
        <Welcome
          tick={tick}
          interval={interval}
          navLeft={navLeft}
          onNav={onNav}
          onInterval={onInterval}
          close={() => setWelcome(false)}
        />
      )}
      <div className="a-about-inner">
        <div className="a-about-hero">
          <Hundred tick={tick} />
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
          <LinkCard
            href="https://argustask.com/"
            mark={<img className="a-link-mark" src="/assets/argus.svg" width="40" height="40" alt="" />}
            title="Argus 官网"
            address="argustask.com"
            pages={[
              ["download", "下载", "https://argustask.com" + localized(language(), "/download/")],
              ["guide", "使用指南", "https://argustask.com/docs/guide/"],
              ["notes", "更新日志", "https://argustask.com" + localized(language(), "/changelog/")],
            ]}
          />
          <LinkCard
            repo
            href="https://github.com/Ray-lea/Argus"
            mark={
              <span className="a-link-mark">
                <Icon name="github" size={24} />
              </span>
            }
            title="GitHub 仓库"
            address="github.com/Ray-lea/Argus"
            pages={[
              ["issue", "问题反馈", "https://github.com/Ray-lea/Argus/issues/new/choose"],
              ["discussion", "讨论区", "https://github.com/Ray-lea/Argus/discussions"],
              ["release", "发行版", "https://github.com/Ray-lea/Argus/releases"],
            ]}
          />
        </div>
        <h2>项目与支持</h2>
        <Setting icon="document" title="诊断信息" desc="复制当前版本、运行状态、硬件与显示设置，便于粘贴到问题报告。">
          <Button onClick={()=>onCopy([`Argus ${appVersion} (website demo)`,"Windows 11 · x64",translate("管理员"),machine.cpu,machine.gpu,"32 GB · 1920 × 1080"].join("\n"),"诊断信息")}>复制诊断信息</Button>
        </Setting>
        <Setting icon="chat" title="交流与反馈" desc="加入 QQ 群 453803497，反馈问题、提出建议或交流使用心得。">
          <Button onClick={() => onCopy("453803497", "群号")}>复制群号</Button>
          <Button tone="accent">加入 QQ 群</Button>
        </Setting>
        <Setting icon="channel" title="Telegram 频道" desc="加入 Telegram 频道 t.me/argustask，获取版本更新与项目动态。">
          <Button onClick={() => onCopy("https://t.me/argustask", "频道链接")}>复制链接</Button>
          <Button tone="accent">加入频道</Button>
        </Setting>
        <Setting icon="sparkle" title="首次启动引导" desc="重新查看常用设置与使用提示。">
          <Button onClick={() => setWelcome(true)}>打开引导</Button>
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
          desc="仅保存在本机，不记录命令行、凭据或文件内容。公开附上日志前，请检查路径中的用户名。"
        >
          <Button onClick={() => onSay("演示窗口不会打开日志文件夹。")}>打开日志文件夹</Button>
        </Setting>
      </div>
    </div>
  );
}
