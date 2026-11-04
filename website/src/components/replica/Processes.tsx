/** @jsxImportSource @/components/replica/i18n */
"use client";
import {
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
} from "react";
import Curve, { type Motion } from "./Curve";
import { language, translate } from "./i18n";
import { Icon, ProcessIcon, type UiIcon } from "./icons";
import {
  bytes,
  clock,
  coreLoad,
  groupLabel,
  heat,
  logical,
  machine,
  noise,
  percent,
  rate,
  rateCeiling,
  sampleAt,
  samples,
  services,
  thousands,
  totalMemory,
  type Glyph,
  type Group,
  type Process,
  type Reading,
} from "./model";
import { slideIn, useIsoLayoutEffect } from "./widgets";

export type View = "apps" | "tree" | "list";
export type SortKey = "name" | "pid" | "cpu" | "mem" | "read" | "write" | "io" | "gpu" | "net";
export type Device = "cpu" | "memory" | "disk" | "wifi" | "gpu" | "battery";

export interface Row {
  key: string;
  kind: "group" | "unit";
  depth: number;
  label: string;
  /** The faint text after the name: the image name beside a friendly name. */
  detail?: string;
  exe?: string;
  count: number;
  pids: number[];
  glyph?: Glyph;
  group?: Group;
  expandable: boolean;
  expanded: boolean;
  values: Reading;
  suspended?: boolean;
}

type Metric = Exclude<SortKey, "name" | "pid">;
const columns: { key: Exclude<SortKey, "name">; label: string; title: string }[] = [
  { key: "pid", label: "PID", title: "进程 ID" },
  { key: "cpu", label: "CPU", title: "占全部处理器能力的百分比" },
  { key: "mem", label: "内存", title: "专用工作集：进程独占的物理内存" },
  { key: "read", label: "磁盘读取", title: "每秒磁盘读取字节数，不包含网络及其他设备 I/O" },
  { key: "write", label: "磁盘写入", title: "每秒磁盘写入字节数，不包含网络及其他设备 I/O" },
  { key: "io", label: "I/O", title: "读、写及其他 I/O 合计，包含文件、网络和设备访问" },
  { key: "gpu", label: "GPU", title: "进程在最忙的 GPU 引擎上占用的时间比例" },
  { key: "net", label: "网络", title: "每秒发送和接收的字节数" },
];
const metrics: Metric[] = ["cpu", "mem", "read", "write", "io", "gpu", "net"];
const zero: Reading = { cpu: 0, mem: 0, read: 0, write: 0, io: 0, gpu: 0, net: 0 };
function sum(pids: number[], now: Map<number, Reading>): Reading {
  const out = { ...zero };
  for (const pid of pids) {
    const r = now.get(pid);
    if (!r) continue;
    out.cpu += r.cpu;
    out.mem += r.mem;
    out.read += r.read;
    out.write += r.write;
    out.io += r.io;
    out.gpu = Math.max(out.gpu, r.gpu);
    out.net += r.net;
  }
  return out;
}
export const hostedServices = (pid: number) => services.filter((s) => s.pid === pid).map((s) => s.name);
function matches(proc: Process, query: string) {
  if (!query) return true;
  const q = query.trim().toLowerCase();
  if (!q) return true;
  if (/^\d+$/.test(q)) return String(proc.pid).startsWith(q);
  return (
    proc.name.toLowerCase().includes(q) ||
    translate(proc.name).toLowerCase().includes(q) ||
    proc.exe.toLowerCase().includes(q) ||
    proc.path.toLowerCase().includes(q) ||
    hostedServices(proc.pid).some((s) => s.toLowerCase().includes(q))
  );
}

interface BuildOptions {
  view: View;
  sort: SortKey;
  descending: boolean;
  query: string;
  list: Process[];
  now: Map<number, Reading>;
  open: Set<string>;
  closed: Set<string>;
}
// Rows as the app builds them (frontend/model.rs): sorted by name, the list
// falls into the Task Manager categories; any other sort keeps one list.
export function buildRows(o: BuildOptions): Row[] {
  const byPid = new Map(o.list.map((p) => [p.pid, p]));
  // Names sort as they read in the page's language.
  const collator = new Intl.Collator(language() === "en" ? "en" : "zh-CN", { numeric: true });
  const name = (row: Row) => translate(row.label);
  const scale: Record<Metric, number> = {
    cpu: 10,
    gpu: 10,
    mem: 1 / 104858,
    read: 1 / 1024,
    write: 1 / 1024,
    io: 1 / 1024,
    net: 1 / 1024,
  };
  const compare = (a: Row, b: Row) => {
    let c: number;
    if (o.sort === "name") c = collator.compare(name(a), name(b));
    else if (o.sort === "pid") c = (a.count === 1 ? a.pids[0] : 1e9) - (b.count === 1 ? b.pids[0] : 1e9);
    else {
      const k = o.sort;
      c = Math.round(a.values[k] * scale[k]) - Math.round(b.values[k] * scale[k]);
      if (c === 0) return collator.compare(name(a), name(b)) || a.key.localeCompare(b.key);
    }
    if (c === 0) c = a.key.localeCompare(b.key);
    return o.descending ? -c : c;
  };
  const unit = (key: string, pids: number[], depth: number, label: string, detail?: string): Row => {
    const first = byPid.get(pids[0])!;
    return {
      key,
      kind: "unit",
      depth,
      label,
      detail,
      exe: first.exe,
      count: pids.length,
      pids,
      glyph: first.glyph,
      group: first.group,
      expandable: false,
      expanded: false,
      values: sum(pids, o.now),
      suspended: pids.every((pid) => byPid.get(pid)?.suspended),
    };
  };
  const named = (p: Process, depth: number) =>
    unit("p:" + p.pid, [p.pid], depth, p.name, p.name !== p.exe ? p.exe : undefined);
  const out: Row[] = [];
  if (o.view === "tree") {
    const children = new Map<number, Process[]>();
    for (const proc of o.list) {
      const parent = byPid.has(proc.ppid) && proc.ppid !== proc.pid ? proc.ppid : -1;
      children.set(parent, [...(children.get(parent) ?? []), proc]);
    }
    // While searching, the ancestors of matches stay in place.
    const keep = new Set<number>();
    if (o.query.trim()) {
      for (const proc of o.list.filter((p) => matches(p, o.query))) {
        let at: Process | undefined = proc;
        while (at && !keep.has(at.pid)) {
          keep.add(at.pid);
          at = byPid.get(at.ppid);
        }
      }
    }
    const walk = (parent: number, depth: number) => {
      const rows = (children.get(parent) ?? [])
        .filter((proc) => !o.query.trim() || keep.has(proc.pid))
        .map((proc) => {
          const row = named(proc, depth);
          row.expandable = children.has(proc.pid);
          row.expanded = row.expandable && (!o.closed.has(row.key) || !!o.query.trim());
          return row;
        });
      for (const row of rows.sort(compare)) {
        out.push(row);
        if (row.expanded) walk(row.pids[0], depth + 1);
      }
    };
    walk(-1, 0);
    return out;
  }
  let units: Row[];
  if (o.view === "apps") {
    const byApp = new Map<string, Process[]>();
    for (const proc of o.list) {
      const key = proc.group + "/" + proc.exe + "/" + (proc.exe === "svchost.exe" ? proc.pid : "");
      byApp.set(key, [...(byApp.get(key) ?? []), proc]);
    }
    units = [];
    for (const members of byApp.values()) {
      const root = members.find((m) => !members.some((n) => n.pid === m.ppid)) ?? members[0];
      const q = o.query.trim().toLowerCase();
      // A search that names the app shows all of it; otherwise only the matches.
      const named = !q || root.name.toLowerCase().includes(q) || root.exe.toLowerCase().includes(q);
      const shown = named ? members : members.filter((m) => matches(m, o.query));
      if (!shown.length) continue;
      const pids = [root.pid, ...members.filter((m) => m !== root).map((m) => m.pid)];
      const row = unit("a:" + root.exe + ":" + root.pid, pids, 0, root.name, root.name !== root.exe ? root.exe : undefined);
      row.expandable = pids.length > 1;
      row.expanded = row.expandable && (o.open.has(row.key) || shown.length < members.length);
      (row as Row & { shown?: number[] }).shown = shown.map((m) => m.pid);
      units.push(row);
    }
  } else {
    units = o.list.filter((p) => matches(p, o.query)).map((p) => named(p, 0));
  }
  const pushUnit = (row: Row & { shown?: number[] }) => {
    out.push(row);
    if (!row.expanded) return;
    const members = (row.shown ?? row.pids)
      .map((pid) => unit("p:" + pid, [pid], 1, byPid.get(pid)!.exe))
      .sort(compare);
    out.push(...members);
  };
  if (o.sort === "name") {
    for (const group of ["app", "background", "windows"] as Group[]) {
      const members = units.filter((u) => u.group === group).sort(compare);
      if (!members.length) continue;
      const key = "g:" + group;
      const expanded = !o.closed.has(key) || !!o.query.trim();
      out.push({
        key,
        kind: "group",
        depth: 0,
        label: groupLabel[group],
        count: members.length,
        pids: members.flatMap((m) => m.pids),
        expandable: true,
        expanded,
        values: sum(
          members.flatMap((m) => m.pids),
          o.now,
        ),
      });
      if (expanded) members.forEach(pushUnit);
    }
  } else units.sort(compare).forEach(pushUnit);
  return out;
}

export function useSystemHistory(tick: number) {
  return useMemo(() => {
    const points = Array.from({ length: samples }, (_, i) => sampleAt(tick - samples + 1 + i));
    return {
      cpu: points.map((p) => p.cpu),
      kernel: points.map((p) => p.kernel),
      mem: points.map((p) => p.mem),
      disk: points.map((p) => p.read + p.write),
      read: points.map((p) => p.read),
      write: points.map((p) => p.write),
      active: points.map((p) => p.active),
      rx: points.map((p) => p.rx),
      tx: points.map((p) => p.tx),
      net: points.map((p) => p.rx + p.tx),
      gpu: points.map((p) => p.gpu),
    };
  }, [tick]);
}
export const cpuTemperature = (t: number) => 62 + sampleAt(t).cpu * 0.32 + noise(31, t) * 2;
export const cpuPower = (t: number) => 28 + sampleAt(t).cpu * 0.82 + noise(32, t) * 4;
export const gpuTemperature = (t: number) => 50 + noise(7, t) * 4;
export const gpuPower = (t: number) => 18 + sampleAt(t).gpu * 0.2;
const split = (text: string): [string, string] => {
  const at = text.indexOf(" ");
  return at < 0 ? [text, ""] : [text.slice(0, at), text.slice(at + 1)];
};

interface VitalProps {
  tick: number;
  interval: number;
  running: boolean;
  motion: Motion;
  onOpen: (device: Device) => void;
}
export function Vitals({ tick, interval, running, motion, onOpen }: VitalProps) {
  const s = sampleAt(tick);
  const h = useSystemHistory(tick);
  const curve = { tick, interval, running, motion };
  const used = (s.mem / 100) * totalMemory;
  const diskMax = rateCeiling(Math.max(...h.disk));
  const netMax = rateCeiling(Math.max(...h.net), true);
  const [diskValue, diskUnit] = s.read + s.write < 1 ? ["0", "KB/s"] : split(rate(s.read + s.write));
  const [netValue, netUnit] = s.rx + s.tx < 1 ? ["0", "KB/s"] : split(rate(s.rx + s.tx));
  const cards: {
    id: Device;
    name: string;
    color: string;
    values: number[];
    max: number;
    figure: string;
    unit: string;
    aside?: ReactNode;
    metrics?: string[];
  }[] = [
    {
      id: "cpu",
      name: "CPU",
      color: "var(--a-cpu)",
      values: h.cpu,
      max: 100,
      figure: String(Math.round(s.cpu)),
      unit: "%",
      metrics: [
        `${clock("p", tick).toFixed(2)} GHz`,
        `${Math.round(cpuTemperature(tick))} °C`,
        `${Math.round(cpuPower(tick))} W`,
      ],
    },
    {
      id: "memory",
      name: "内存",
      color: "var(--a-mem)",
      values: h.mem,
      max: 100,
      figure: (used / 1024 ** 3).toFixed(1),
      unit: "/ 32.0 GB",
      aside: `${Math.round(s.mem)}%`,
    },
    {
      id: "disk",
      name: "磁盘",
      color: "var(--a-io)",
      values: h.disk,
      max: diskMax,
      figure: diskValue,
      unit: diskUnit,
      aside: `活动 ${Math.round(s.active)}%`,
    },
    {
      id: "wifi",
      name: "网络",
      color: "var(--a-net)",
      values: h.net,
      max: netMax,
      figure: netValue,
      unit: netUnit,
      aside: `收 ${rate(s.rx)} 发 ${rate(s.tx)}`,
    },
    {
      id: "gpu",
      name: "GPU",
      color: "var(--a-gpu)",
      values: h.gpu,
      max: 100,
      figure: String(Math.round(s.gpu)),
      unit: "%",
      aside: (
        <>
          <i>{Math.round(gpuTemperature(tick))} °C</i>
          <i>{Math.round(gpuPower(tick))} W</i>
        </>
      ),
    },
  ];
  return (
    <div className="a-vitals" role="group" aria-label="系统概况">
      {cards.map((card) => {
        const open = `在“性能”中查看${card.name}`;
        return (
          <button
            key={card.id}
            type="button"
            className={"a-vital" + (card.id === "cpu" ? " is-cpu" : "")}
            onClick={() => onOpen(card.id)}
            aria-label={open}
            title={open}
          >
            <span className="a-vital-plot">
              <Curve series={[{ values: card.values, color: card.color }]} max={card.max} {...curve} />
            </span>
            <span className="a-vital-head">
              <span>{card.name}</span>
              {card.id === "cpu" ? (
                <span className="a-cores" title="每个逻辑处理器的使用率" style={{ "--glide": `${interval}ms` } as CSSProperties}>
                  {logical.map((core) => (
                    <i key={core.index} style={{ "--v": (coreLoad(core.index, tick) / 100).toFixed(3) } as CSSProperties} />
                  ))}
                </span>
              ) : (
                <span className="a-vital-aside">{card.aside}</span>
              )}
            </span>
            <span className="a-figure">
              <b>{card.figure}</b>
              <small>{card.unit}</small>
              {card.metrics && (
                <span className="a-figure-metrics">
                  {card.metrics.map((m) => (
                    <span key={m.slice(-3)}>{m}</span>
                  ))}
                </span>
              )}
            </span>
          </button>
        );
      })}
    </div>
  );
}

interface TableProps {
  tick: number;
  rows: Row[];
  total: number;
  sort: SortKey;
  descending: boolean;
  selected: string | null;
  filtered: boolean;
  efficiency: Set<number>;
  onSort: (key: SortKey) => void;
  onToggle: (row: Row) => void;
  onSelect: (key: string | null) => void;
  onMenu: (event: MouseEvent | KeyboardEvent, row: Row) => void;
  onEnd: (row: Row, confirm: boolean) => void;
  onDetails: () => void;
  onHold: (hold: boolean) => void;
  onHeaderMenu: (event: MouseEvent) => void;
}
const kindOf = (key: Metric) => (key === "cpu" || key === "gpu" ? "percent" : key === "mem" ? "memory" : "rate");
function cell(key: Metric, v: Reading) {
  const value = v[key];
  const kind = kindOf(key);
  const text = kind === "percent" ? percent(value) : kind === "memory" ? bytes(value) : rate(value);
  const dim = kind === "percent" ? value < 0.05 : kind === "rate" ? value < 1 : false;
  return { text, alpha: heat(kind, value), dim };
}
export function ProcessTable(t: TableProps) {
  const body = useRef<HTMLDivElement>(null);
  const reveal = (key: string) =>
    body.current?.querySelector(`[data-key="${CSS.escape(key)}"]`)?.scrollIntoView({ block: "nearest" });
  const keyed = (event: KeyboardEvent<HTMLDivElement>) => {
    const units = t.rows.filter((r) => r.kind === "unit");
    const at = units.findIndex((r) => r.key === t.selected);
    const row = at >= 0 ? units[at] : null;
    const move = (index: number) => {
      const next = units[Math.max(0, Math.min(units.length - 1, index))];
      if (!next) return;
      t.onSelect(next.key);
      reveal(next.key);
    };
    const page = Math.max(1, Math.floor((body.current?.clientHeight ?? 300) / 30) - 1);
    switch (event.key) {
      case "ArrowDown":
      case "ArrowUp":
        event.preventDefault();
        move(at < 0 ? 0 : at + (event.key === "ArrowDown" ? 1 : -1));
        return;
      case "PageDown":
      case "PageUp":
        event.preventDefault();
        move((at < 0 ? 0 : at) + (event.key === "PageDown" ? page : -page));
        return;
      case "Home":
      case "End":
        event.preventDefault();
        move(event.key === "Home" ? 0 : units.length - 1);
        return;
    }
    if (!row) return;
    if (event.key === "Delete" || (event.altKey && event.key.toLowerCase() === "e")) {
      event.preventDefault();
      t.onEnd(row, !event.shiftKey);
    } else if (event.key === "Enter") {
      event.preventDefault();
      t.onDetails();
    } else if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
      event.preventDefault();
      t.onMenu(event, row);
    } else if (event.key === "ArrowRight" && row.expandable) {
      event.preventDefault();
      if (!row.expanded) t.onToggle(row);
      else move(at + 1);
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      if (row.expandable && row.expanded) t.onToggle(row);
      else if (row.depth > 0) {
        const index = t.rows.indexOf(row);
        const parent = t.rows.slice(0, index).reverse().find((r) => r.kind === "unit" && r.depth < row.depth);
        if (parent) {
          t.onSelect(parent.key);
          reveal(parent.key);
        }
      }
    }
  };
  const sortMark = (key: SortKey) =>
    t.sort === key ? <Icon name={t.descending ? "chevronDown" : "chevronUp"} size={12} /> : null;
  const ariaSort = (key: SortKey) => (t.sort === key ? (t.descending ? "descending" : "ascending") : "none");
  return (
    <div className="a-table" role="grid" aria-label="进程" aria-rowcount={t.rows.length + 1}>
      <div
        className="a-thead"
        role="row"
        onContextMenu={(event) => {
          event.preventDefault();
          t.onHeaderMenu(event);
        }}
      >
        <button
          type="button"
          role="columnheader"
          className="a-th a-col-name"
          aria-sort={ariaSort("name")}
          title="应用名称或文件描述；次级文字显示可执行文件名。"
          data-tour="col-name"
          onClick={() => t.onSort("name")}
        >
          <span className="a-th-total">
            {t.filtered && <Icon name="filter" size={13} />}
            {t.total} 个进程
          </span>
          <span className="a-th-label">
            <span>名称</span>
            {sortMark("name")}
          </span>
        </button>
        {columns.map((c) => (
          <button
            key={c.key}
            type="button"
            role="columnheader"
            className={`a-th is-end a-col-${c.key}`}
            title={c.title}
            aria-sort={ariaSort(c.key)}
            data-tour={"col-" + c.key}
            onClick={() => t.onSort(c.key)}
          >
            <span className="a-th-total">{c.key === "pid" ? "" : <TotalFor column={c.key} tick={t.tick} />}</span>
            <span className="a-th-label">
              <span>{c.label}</span>
              {sortMark(c.key)}
            </span>
          </button>
        ))}
      </div>
      <div
        className="a-tbody a-scroll"
        ref={body}
        tabIndex={0}
        role="rowgroup"
        onKeyDown={keyed}
        onPointerEnter={() => t.onHold(true)}
        onPointerLeave={() => t.onHold(false)}
      >
        {t.rows.length === 0 && <p className="a-empty">没有匹配的进程</p>}
        {t.rows.map((row) =>
          row.kind === "group" ? (
            <div
              key={row.key}
              role="row"
              data-group={row.key}
              aria-expanded={row.expanded}
              className={"a-tr a-group" + (row.expanded ? "" : " is-folded")}
              onClick={() => t.onToggle(row)}
            >
              <span role="gridcell" className="a-td a-td-name a-col-name">
                <span className="a-fold" aria-expanded={row.expanded}>
                  <Icon name="chevronRight" size={12} />
                </span>
                <span className="a-group-name">{row.label}</span>
                <span className="a-group-count">{row.count}</span>
                <span className="a-group-rule" />
              </span>
              <span role="gridcell" className="a-td a-col-pid" />
              {metrics.map((k) => (
                <span role="gridcell" key={k} className={`a-td is-end a-col-${k}`}>
                  {cell(k, row.values).text}
                </span>
              ))}
            </div>
          ) : (
            <div
              key={row.key}
              data-key={row.key}
              role="row"
              aria-selected={t.selected === row.key}
              aria-expanded={row.expandable ? row.expanded : undefined}
              className={"a-tr" + (t.selected === row.key ? " is-selected" : "")}
              onClick={(event) => {
                t.onSelect(t.selected === row.key ? null : row.key);
                if (event.detail === 2 && row.expandable) t.onToggle(row);
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                t.onSelect(row.key);
                t.onMenu(event, row);
              }}
            >
              <span
                role="gridcell"
                className="a-td a-td-name a-col-name"
                style={{ paddingLeft: 6 + row.depth * 20 }}
                title={row.label + "\n" + (row.exe ?? "")}
              >
                {row.expandable ? (
                  <button
                    type="button"
                    className="a-fold"
                    aria-label={row.expanded ? "折叠" : "展开"}
                    aria-expanded={row.expanded}
                    tabIndex={-1}
                    onClick={(event) => {
                      event.stopPropagation();
                      t.onToggle(row);
                    }}
                  >
                    <Icon name="chevronRight" size={12} />
                  </button>
                ) : (
                  <span className="a-fold-gap" />
                )}
                <ProcessIcon glyph={row.glyph ?? "app"} />
                <span
                  className={
                    "a-name" + (row.count > 1 ? " is-strong" : "") + (row.suspended ? " is-muted" : "")
                  }
                >
                  {row.label}
                </span>
                {row.count > 1 && <span className="a-count">{row.count}</span>}
                {row.detail && <span className="a-exe">{row.detail}</span>}
                {row.pids.some((pid) => t.efficiency.has(pid)) && (
                  <span title="效率模式" className="a-leaf">
                    <Icon name="leaf" size={12} />
                  </span>
                )}
                {row.suspended && <span className="a-tag">已暂停</span>}
              </span>
              <span role="gridcell" className="a-td is-end is-faint a-col-pid">
                {row.count === 1 ? row.pids[0] : ""}
              </span>
              {metrics.map((k) => {
                const c = cell(k, row.values);
                return (
                  <span
                    role="gridcell"
                    key={k}
                    className={`a-td is-end a-col-${k}` + (c.dim ? " is-faint" : "") + (c.alpha ? " is-heat" : "")}
                    style={c.alpha ? ({ "--h": c.alpha.toFixed(3) } as CSSProperties) : undefined}
                  >
                    {c.text}
                  </span>
                );
              })}
            </div>
          ),
        )}
      </div>
    </div>
  );
}

// Column totals come from the system sample, as in the app's header band.
function TotalFor({ column, tick }: { column: Metric; tick: number }) {
  const s = sampleAt(tick);
  switch (column) {
    case "cpu":
      return <>{Math.round(s.cpu)}%</>;
    case "mem":
      return <>{Math.round(s.mem)}%</>;
    case "read":
      return <>{rate(s.read)}</>;
    case "write":
      return <>{rate(s.write)}</>;
    case "io":
      return <>{rate((s.read + s.write) * 1.12 + s.rx * 0.05)}</>;
    case "gpu":
      return <>{Math.round(s.gpu)}%</>;
    case "net":
      return <>{rate(s.rx + s.tx)}</>;
  }
}

interface DetailsProps {
  entering?: boolean;
  row: Row | null;
  list: Process[];
  tick: number;
  interval: number;
  running: boolean;
  motion: Motion;
  history: (pids: number[]) => { cpu: number[]; mem: number[] };
  efficiency: Set<number>;
  priority: Map<number, string>;
  onDeselect: () => void;
  onEnd: (row: Row, confirm: boolean, tree?: boolean) => void;
  onSelectPid: (pid: number) => void;
  onDevice: (device: Device) => void;
  onService: (name: string) => void;
  onCopy: (text: string, what: string) => void;
}
const cpuTime = (seconds: number) =>
  `${Math.floor(seconds / 3600)}:${String(Math.floor((seconds % 3600) / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
function Facts({ rows }: { rows: [string, ReactNode][] }) {
  return (
    <dl className="a-facts">
      {rows.map(([name, value]) => (
        <div key={name}>
          <dt>{name}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}
function PathRow({ path, onCopy }: { path: string; onCopy: (text: string, what: string) => void }) {
  return (
    <div className="a-path">
      <p className="a-code">{path}</p>
      <div>
        <button type="button" className="a-icon-btn" title="打开文件位置" aria-label="打开文件位置">
          <Icon name="folder" size={14} />
        </button>
        <button type="button" className="a-icon-btn" title="复制路径" aria-label="复制路径" onClick={() => onCopy(path, "路径")}>
          <Icon name="copy" size={14} />
        </button>
      </div>
    </div>
  );
}
// A details card settles into place as it opens: up from 6 px below, fading in.
const settle = (el: HTMLDivElement | null) => slideIn(el, { y: 6, fade: true });
export function Details(d: DetailsProps) {
  const row = d.row;
  const proc = row ? d.list.find((p) => p.pid === row.pids[0]) : undefined;
  // The pane, opened, moves in from the right.
  const pane = useRef<HTMLElement>(null);
  useIsoLayoutEffect(() => {
    if (d.entering) slideIn(pane.current, { x: 24 });
  }, [d.entering]);
  return (
    <aside className="a-details" ref={pane} aria-label="详细信息面板" data-tour="details">
      {row && proc ? (
        <ProcessDetails key={row.key} {...d} row={row} proc={proc} />
      ) : (
        <Overview key="overview" onDevice={d.onDevice} onCopy={d.onCopy} />
      )}
    </aside>
  );
}
function ProcessDetails(d: DetailsProps & { row: Row; proc: Process }) {
  const { row, proc } = d;
  const group = row.count > 1;
  const members = row.pids.map((pid) => d.list.find((p) => p.pid === pid)).filter(Boolean) as Process[];
  const parent = d.list.find((p) => p.pid === proc.ppid);
  const h = d.history(row.pids);
  const memMax = Math.max(...h.mem) * 1.25 || 1;
  const curve = { tick: d.tick, interval: d.interval, running: d.running, motion: d.motion };
  const hosted = services.filter((s) => s.pid && row.pids.includes(s.pid));
  const total = (key: "threads" | "handles") => thousands(members.reduce((a, p) => a + p[key], 0));
  const net = row.values.net;
  const resources: [string, ReactNode][] = [
    ["专用工作集", bytes(row.values.mem)],
    ["工作集", bytes(row.values.mem * 1.38)],
    ["提交大小", bytes(row.values.mem * 1.17)],
    ["磁盘读取", rate(row.values.read)],
    ["磁盘写入", rate(row.values.write)],
    ["GPU", row.values.gpu >= 0.05 ? `${percent(row.values.gpu)}%（3D）` : "0%"],
    ["网络", `发送 ${rate(net * 0.22)}，接收 ${rate(net * 0.78)}`],
    ["线程", total("threads")],
    ["句柄", total("handles")],
  ];
  return (
    <div className="a-details-body" ref={settle}>
      <div className="a-details-scroll a-scroll">
        <header className="a-details-head">
          <span className="a-head-glyph">
            <ProcessIcon glyph={proc.glyph} size={32} />
          </span>
          <div className="a-details-title">
            <h3 title={row.label}>{row.label}</h3>
            <p>{group ? `${proc.exe} · ${row.count} 个进程` : proc.exe}</p>
          </div>
          <button type="button" className="a-icon-btn" aria-label="取消选中" title="取消选中" onClick={d.onDeselect}>
            <Icon name="dismiss" size={14} />
          </button>
        </header>
        <div className="a-minis">
          <div className="a-mini">
            <span>CPU</span>
            <b>{percent(row.values.cpu)}%</b>
            <Curve series={[{ values: h.cpu, color: "var(--a-cpu)" }]} max={100} {...curve} />
          </div>
          <div className="a-mini">
            <span>内存</span>
            <b>{bytes(row.values.mem)}</b>
            <Curve series={[{ values: h.mem, color: "var(--a-mem)" }]} max={memMax} {...curve} />
          </div>
        </div>
        {hosted.length > 0 && (
          <>
            <h4>运行的服务（{hosted.length}）</h4>
            <div className="a-pick-list">
              {hosted.map((s) => (
                <button type="button" key={s.name} title={s.description} onClick={() => d.onService(s.name)}>
                  <span className="is-main">{s.display}</span>
                  <span className="is-side">{s.name}</span>
                </button>
              ))}
            </div>
          </>
        )}
        {group ? (
          <>
            <h4>资源</h4>
            <Facts
              rows={[
                ...resources,
                ["CPU 时间", cpuTime(members.reduce((a, p) => a + p.cpuTime, 0))],
                ["最早启动", members.map((p) => p.started).sort()[0]],
              ]}
            />
            <h4>进程（{row.count}）</h4>
            <div className="a-pick-list">
              {members
                .map((p) => ({ p, r: d.history([p.pid]) }))
                .sort((a, b) => (b.r.cpu.at(-1) ?? 0) - (a.r.cpu.at(-1) ?? 0))
                .map(({ p, r }) => {
                  return (
                    <button
                      type="button"
                      key={p.pid}
                      title={`查看 PID ${p.pid} 的详细信息`}
                      onClick={() => d.onSelectPid(p.pid)}
                    >
                      <span className="is-main">PID {p.pid}</span>
                      <span className="is-num">{percent(r.cpu.at(-1) ?? 0)}%</span>
                      <span className="is-num is-wide">{bytes(r.mem.at(-1) ?? p.mem)}</span>
                    </button>
                  );
                })}
            </div>
            <h4>文件位置</h4>
            {[...new Set(members.map((p) => p.path))].map((path) => (
              <PathRow key={path} path={path} onCopy={d.onCopy} />
            ))}
          </>
        ) : (
          <>
            <h4>进程</h4>
            <Facts
              rows={[
                ["PID", proc.pid],
                [
                  "父进程",
                  parent ? (
                    <button type="button" className="a-link" onClick={() => d.onSelectPid(parent.pid)}>
                      {parent.exe}（{parent.pid}）
                    </button>
                  ) : (
                    `无（PID ${proc.ppid}）`
                  ),
                ],
                ["启动时间", proc.started],
                ["CPU 时间", cpuTime(proc.cpuTime)],
                ["用户", proc.user],
                ["完整性", proc.integrity],
                ["架构", "64 位 (x64)"],
                ["优先级", d.priority.get(proc.pid) ?? "正常"],
                ["效率模式", d.efficiency.has(proc.pid) ? "开启" : "关闭"],
                ["会话", proc.group === "windows" && proc.user.startsWith("NT") ? "0" : "1"],
              ]}
            />
            <h4>资源</h4>
            <Facts rows={resources} />
            <h4>文件位置</h4>
            <PathRow path={proc.path} onCopy={d.onCopy} />
            {proc.args && (
              <>
                <h4>命令行</h4>
                <p className="a-code">
                  "{proc.path}" {proc.args}
                </p>
              </>
            )}
          </>
        )}
      </div>
      <footer className="a-details-foot">
        <button type="button" className="a-btn is-danger" onClick={() => d.onEnd(row, true)}>
          {group ? "全部结束" : "结束进程"}
        </button>
        {!group && (
          <button type="button" className="a-btn" onClick={() => d.onEnd(row, true, true)}>
            结束进程树
          </button>
        )}
      </footer>
    </div>
  );
}

const parts: { device: Device; icon: UiIcon; color: string; kind: string; name: string; detail: string }[] = [
  {
    device: "cpu",
    icon: "chip",
    color: "var(--a-cpu)",
    kind: "处理器",
    name: machine.cpu,
    detail: "14 核（6 性能核 + 8 能效核） · 20 线程 · 基准 2.50 GHz",
  },
  { device: "memory", icon: "memory", color: "var(--a-mem)", kind: "内存", name: "32 GB DDR5", detail: "5600 MT/s · 已用插槽 2 / 2 · SODIMM" },
  { device: "disk", icon: "disk", color: "var(--a-io)", kind: "存储", name: machine.disk, detail: "1.8 TB · SSD (NVMe) · C:" },
  { device: "wifi", icon: "network", color: "var(--a-net)", kind: "Wi-Fi", name: machine.wifi, detail: "WLAN · 1.2 Gbps" },
  { device: "gpu", icon: "gpu", color: "var(--a-gpu)", kind: "显卡", name: machine.gpu, detail: "8.0 GB 专用显存" },
  { device: "battery", icon: "battery", color: "var(--a-bat)", kind: "电池", name: "Contoso CT-4410", detail: "设计容量 80 Wh · 健康度 90%" },
];
function Overview({ onDevice, onCopy }: { onDevice: (device: Device) => void; onCopy: (text: string, what: string) => void }) {
  const [visible, setVisible] = useState(true);
  const text = ["本机配置", ...parts.map((p) => `${p.kind}：${p.name}\n${p.detail}`)].join("\n\n");
  return (
    <div className="a-details-body" ref={settle}>
      <div className="a-details-scroll a-scroll">
        <header className="a-details-head is-center">
          <span className="a-head-glyph">
            <Icon name="computer" size={32} />
          </span>
          <div className="a-details-title">
            <h3>本机配置</h3>
          </div>
          <div className="a-details-tools">
            <button type="button" className="a-icon-btn" title="复制配置为文字" aria-label="复制配置为文字" onClick={() => onCopy(text, "本机配置")}>
              <Icon name="copy" />
            </button>
            <button
              type="button"
              className="a-icon-btn"
              title={visible ? "隐藏本机配置" : "显示本机配置"}
              aria-label={visible ? "隐藏本机配置" : "显示本机配置"}
              aria-pressed={!visible}
              onClick={() => setVisible(!visible)}
            >
              <Icon name={visible ? "eye" : "eyeOff"} />
            </button>
          </div>
        </header>
        <div className="a-tips">
          <p>
            <Icon name="pointer" size={14} />
            <span>在列表中单击应用或进程，即可在此查看其详细信息；再次单击取消选中。</span>
          </p>
          <p>
            <Icon name="panel" size={14} />
            <span>
              详细信息面板默认常驻，单击窗口右上角的
              <span className="a-key" aria-label="详细信息面板按钮">
                <Icon name="panel" size={12} />
              </span>
              可收起。
            </span>
          </p>
        </div>
        {visible && (
          <ul className="a-hardware">
            {parts.map((part) => (
              <li key={part.device}>
                <button type="button" onClick={() => onDevice(part.device)}>
                  <span className="a-chip" style={{ "--c": part.color } as CSSProperties}>
                    <Icon name={part.icon} />
                  </span>
                  <span className="a-hardware-text">
                    <small>{part.kind}</small>
                    <b>{part.name}</b>
                    <span>{part.detail}</span>
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
