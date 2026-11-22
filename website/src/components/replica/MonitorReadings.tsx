/** @jsxImportSource @/components/replica/i18n */
"use client";
import { Icon } from "./icons";
import type { CSSProperties } from "react";
import { machine, sampleAt, rate } from "./model";
import { byMetric, metricValue } from "./metrics";
import { monitorGroups, setMonitor, useShared, type MonitorKind } from "./store";
export const taskbarStyles = ["标签", "曲线", "胶囊", "大数字"];
export const desktopStyles = ["列表", "曲线", "仪表"];
const names: Record<string, string> = {
  cpu: "利用率",
  memoryPercent: "占比",
  mem: "用量",
  gpu: "利用率",
  temp: "温度",
  clock: "频率",
  power: "功耗",
  gpuTemp: "温度",
  gpuMem: "显存",
  gpuPower: "功耗",
  gpuClock: "频率",
  gpuFan: "风扇",
  disk: "读写速度",
  diskTemp: "温度",
  diskActive: "活动时间",
  net: "收发速率",
};
const groupLabel: Record<string, string> = { 磁盘智能显示: "磁盘", 全部网卡: "网络" };
export function MiniCurve({ id, tick }: { id: string; tick: number }) {
  const m = byMetric[id],
    vals = Array.from({ length: 36 }, (_, i) => m.read(sampleAt(tick + i - 35)));
  const max = m.unit === "%" ? 100 : Math.max(...vals, 1) * 1.08;
  const path = vals.map((v, i) => i * 2 + "," + (22 - (21 * v) / max).toFixed(1)).join(" ");
  return (
    <svg className="w-mini" viewBox="0 0 70 22" preserveAspectRatio="none" aria-hidden="true">
      <polygon points={"0,22 " + path + " 70,22"} fill="currentColor" opacity=".15" />
      <polyline points={path} fill="none" stroke="currentColor" strokeWidth="1.3" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}
export function MonitorFace({
  kind,
  style,
  tick,
  preview = false,
}: {
  kind: MonitorKind;
  style: string;
  tick: number;
  preview?: boolean;
}) {
  const cfg = useShared().widgets[kind];
  const ids = cfg.order.filter((id) => cfg.keys.includes(id));
  const seen = new Set<string>();
  return (
    <div
      className="w-face"
      data-kind={kind}
      data-style={
        (
          {
            标签: "label",
            曲线: "curve",
            胶囊: "capsule",
            大数字: "number",
            列表: "list",
            仪表: "gauge",
            横条: "strip",
          } as Record<string, string>
        )[style]
      }
    >
      {(preview ? (ids.length ? ids.slice(0, 2) : ["cpu", "memoryPercent"]) : ids).map((id) => {
        const m = byMetric[id],
          group = monitorGroups.find((g) => g.keys.includes(id))!;
        const first = !seen.has(group.name);
        seen.add(group.name);
        const name = first && group.name !== "其他" ? (groupLabel[group.name] ?? group.name) : (names[id] ?? m.name);
        const value = m.read(sampleAt(tick)),
          level = m.unit === "%" || m.unit === "°C" ? value : Math.min(95, 25 + sampleAt(tick).cpu);
        return (
          <div className="w-reading" key={id} style={{ "--metric": m.color, "--level": level + "%" } as CSSProperties}>
            {style === "曲线" && <MiniCurve id={id} tick={tick} />}
            <small>{name}</small>
            {m.second ? (
              <b className="w-rate">
                <em>{id === "net" ? "↓" : "读"}</em> {rate(value)} <em>{id === "net" ? "↑" : "写"}</em>{" "}
                {rate(m.second(sampleAt(tick)))}
              </b>
            ) : (
              <b data-high={((m.unit === "%" || m.unit === "°C") && value >= 80) || undefined}>
                {metricValue(m, tick)}
              </b>
            )}
          </div>
        );
      })}
    </div>
  );
}
export function Styles({ kind }: { kind: MonitorKind }) {
  const cfg = useShared().widgets[kind];
  return (
    <div className="w-styles">
      {(kind === "desktop" ? desktopStyles : taskbarStyles).map((s) => (
        <button type="button" key={s} aria-pressed={cfg.style === s} onClick={() => setMonitor(kind, { style: s })}>
          <MonitorFace kind={kind} style={s} tick={0} preview />
          <strong>
            <i />
            {s}
          </strong>
        </button>
      ))}
    </div>
  );
}
const deviceHints: Record<string, string> = {
  CPU: machine.cpu,
  内存: "32.0 GB",
  GPU: machine.gpu,
  磁盘智能显示: machine.disk,
  全部网卡: machine.wifi,
};
export function Readings({ kind }: { kind: MonitorKind }) {
  const cfg = useShared().widgets[kind];
  const ordered = [...monitorGroups].sort(
    (a, b) =>
      Math.min(...a.keys.map((k) => cfg.order.indexOf(k))) - Math.min(...b.keys.map((k) => cfg.order.indexOf(k))),
  );
  const reorder = (move: string[], before: string) => {
    const next = cfg.order.filter((k) => !move.includes(k));
    next.splice(next.indexOf(before), 0, ...move);
    setMonitor(kind, { order: next });
  };
  return (
    <div className="w-readings">
      {ordered.map((g) => (
        <div key={g.name} className="w-group">
          <b
            draggable
            onDragStart={(e) => e.dataTransfer.setData("argus-category", g.name)}
            onDragOver={(e) => e.preventDefault()}
            onDrop={(e) => {
              const source = ordered.find((o) => o.name === e.dataTransfer.getData("argus-category"));
              if (source && source !== g)
                reorder(
                  cfg.order.filter((k) => source.keys.includes(k)),
                  cfg.order.find((k) => g.keys.includes(k))!,
                );
            }}
          >
            <Icon name="grip" size={13} />
            <span>
              {g.name}
              {deviceHints[g.name] && <small>{deviceHints[g.name]}</small>}
            </span>
          </b>
          <div>
            {cfg.order
              .filter((id) => g.keys.includes(id))
              .map((id) => (
                <label
                  draggable
                  key={id}
                  onDragStart={(e) => e.dataTransfer.setData("argus-metric", id)}
                  onDragOver={(e) => e.preventDefault()}
                  onDrop={(e) => {
                    const from = e.dataTransfer.getData("argus-metric");
                    if (g.keys.includes(from) && from !== id) reorder([from], id);
                  }}
                >
                  <input
                    type="checkbox"
                    checked={cfg.keys.includes(id)}
                    onChange={(e) =>
                      setMonitor(kind, {
                        keys: e.target.checked ? [...cfg.keys, id] : cfg.keys.filter((k) => k !== id),
                      })
                    }
                  />
                  {names[id] ?? byMetric[id].name}
                </label>
              ))}
          </div>
        </div>
      ))}
    </div>
  );
}
