/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useEffect, useRef, useState, type PointerEvent } from "react";
import { Icon, ProcessIcon } from "./icons";
import { appVersion, processes, readings, sampleAt, rate } from "./model";
import { byMetric } from "./metrics";
import { store, useShared, setWidgets, setMonitor, type Request } from "./store";
import { MiniCurve, MonitorFace, Styles, Readings } from "./MonitorReadings";
import { Button } from "./widgets";

export function TrayGlyph({ tick, style }: { tick: number; style: string }) {
  const value = Math.round(sampleAt(tick).cpu);
  if (style === "应用图标") return <img src="/assets/argus.svg" width="20" height="20" alt="" />;
  if (style === "数字") return <strong style={{ color: value >= 80 ? "#cd8b20" : undefined }}>{value}</strong>;
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" aria-hidden="true">
      {style === "走势" && <path d="M2 6h20" stroke="currentColor" strokeWidth="2" />}
      {Array.from({ length: 25 }, (_, i) => {
        const x = i % 5,
          y = Math.floor(i / 5),
          on = style === "占比" ? i < Math.round(value / 4) : y < Math.round(sampleAt(tick + x - 4).cpu / 20);
        return (
          <rect
            key={i}
            x={3 + x * 4}
            y={20 - y * (style === "走势" ? 3 : 4)}
            width="2.6"
            height="2.6"
            rx=".6"
            fill={on && (style === "占比" || x === 4) ? (value >= 80 ? "#cd8b20" : "var(--accent)") : "currentColor"}
            opacity={on ? 1 : 0.18}
          />
        );
      })}
    </svg>
  );
}
export function widgetSettings(section: string) {
  setWidgets({ section });
  store.set({ request: { page: "components", at: Date.now() } });
}
export function TrayPanel({ tick, card, close }: { tick: number; card: boolean; close: () => void }) {
  const { widgets: w, paused } = useShared();
  const show = (r: Omit<Request, "at">) => {
    close();
    store.set({ request: { ...r, at: Date.now() } });
  };
  const command = (action: "show" | "pause" | "exit") => {
    close();
    store.set({ command: { action, at: Date.now() } });
  };
  const stats = sampleAt(tick);
  const tiles = [
    ["cpu", "cpu", "CPU", "chip", stats.cpu, Math.round(stats.cpu) + "%", ""],
    [
      "memoryPercent",
      "memory",
      "内存",
      "memory",
      stats.mem,
      Math.round(stats.mem) + "%",
      (stats.mem * 0.32).toFixed(1) + " / 32.0 GB",
    ],
    ["gpu", "gpu", "GPU", "gpu", stats.gpu, Math.round(stats.gpu) + "%", ""],
    ["net", "wifi", "网络", "network", 0, "↓ " + rate(stats.rx), "↑ " + rate(stats.tx)],
  ] as const;
  if (card)
    return (
      <div className="w-popup w-tray-popup is-card" role="tooltip" aria-label="悬停卡片">
        <header>
          <img src="/assets/argus.svg" width="16" height="16" alt="" />
          <b>Argus</b>
        </header>
        <div className="w-card-readings">
          {tiles.map(([id, , name, , load, value, extra]) => (
            <div key={id} style={{ "--metric": byMetric[id].color } as React.CSSProperties}>
              <span>{name}</span>
              {id === "net" ? (
                <b>
                  {value} {extra}
                </b>
              ) : (
                <>
                  <i>
                    <i style={{ width: load + "%" }} />
                  </i>
                  <b>{value}</b>
                </>
              )}
            </div>
          ))}
        </div>
        <footer>
          单击：显示或隐藏窗口
          <br />
          {w.trayMenu === "快捷面板" ? "右键单击：打开快捷面板" : "右键单击：打开菜单"}
        </footer>
      </div>
    );
  if (w.trayMenu === "菜单")
    return (
      <div className="w-popup w-tray-popup is-menu" role="menu">
        <Button onClick={() => command("show")}>打开 Argus</Button>
        <Button onClick={() => command("exit")}>退出 Argus</Button>
      </div>
    );
  return (
    <div className="w-popup w-tray-popup is-panel" role="dialog" aria-label="Argus 快捷面板">
      <header>
        <img src="/assets/argus.svg" width="28" height="28" alt="" />
        <div>
          <b>Argus</b>
          <small>v{appVersion}</small>
        </div>
      </header>
      <div className="w-tiles">
        {tiles.map(([id, device, name, icon, , value, extra]) => (
          <button
            key={id}
            style={{ "--metric": byMetric[id].color } as React.CSSProperties}
            onClick={() => show({ page: "performance", device })}
          >
            <span>
              <Icon name={icon} size={16} />
              {name}
              <small>{extra}</small>
            </span>
            <b>{value}</b>
            <MiniCurve id={id} tick={tick} />
          </button>
        ))}
      </div>
      <div className="w-busy-title">
        <span>占用最高</span>CPU
      </div>
      {[...readings(tick, processes)]
        .sort((a, b) => b[1].cpu - a[1].cpu)
        .slice(0, 3)
        .map(([pid, r]) => {
          const item = processes.find((p) => p.pid === pid)!;
          return (
            <button className="w-busy-process" key={pid} onClick={() => show({ page: "processes", pid })}>
              <ProcessIcon glyph={item.glyph} />
              <span>{item.name}</span>
              <b>{r.cpu.toFixed(1)}%</b>
            </button>
          );
        })}
      <footer className="w-popup-actions">
        <Button icon="genericApp" tone="accent" onClick={() => command("show")}>
          打开 Argus
        </Button>
        <Button
          icon={paused ? "play" : "pause"}
          title={paused ? "继续刷新" : "暂停刷新"}
          onClick={() => command("pause")}
        />
        <Button
          icon="settings"
          title="托盘设置"
          onClick={() => {
            close();
            widgetSettings("托盘");
          }}
        />
        <Button icon="signOut" title="退出 Argus" onClick={() => command("exit")} />
      </footer>
    </div>
  );
}
export function TaskMonitor({ tick, toggle }: { tick: number; toggle: () => void }) {
  const { widgets: w } = useShared(),
    [panel, setPanel] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  useOutside(root, () => setPanel(false), panel);
  if (!w.taskbar.enabled) return null;
  return (
    <div ref={root} className={"w-task-monitor" + (w.taskbar.place === "任务栏左侧" ? " at-start" : "")}>
      <button
        title="任务栏监控"
        onClick={toggle}
        onContextMenu={(e) => {
          e.preventDefault();
          setPanel(!panel);
        }}
      >
        <MonitorFace kind="taskbar" tick={tick} style={w.taskbar.style} />
      </button>
      {panel && (
        <div className="w-popup w-task-panel" role="dialog" aria-label="任务栏监控">
          <b>样式</b>
          <Styles kind="taskbar" />
          <b>显示的读数</b>
          <Readings kind="taskbar" />
          <div className="w-popup-actions">
            <Button
              onClick={() => {
                setPanel(false);
                widgetSettings("任务栏监控");
              }}
            >
              组件设置…
            </Button>
            <Button
              onClick={() => {
                setPanel(false);
                setMonitor("taskbar", { enabled: false });
              }}
            >
              关闭任务栏监控
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
function useOutside(ref: React.RefObject<HTMLElement | null>, close: () => void, open: boolean) {
  useEffect(() => {
    if (!open) return;
    const pointer = (e: globalThis.PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) close();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    document.addEventListener("pointerdown", pointer);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("pointerdown", pointer);
      document.removeEventListener("keydown", key);
    };
  }, [ref, close, open]);
}
export function DesktopMonitor({ tick }: { tick: number }) {
  const { desktop: d } = useShared().widgets,
    [menu, setMenu] = useState(false),
    [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  const root = useRef<HTMLDivElement>(null);
  useOutside(root, () => setMenu(false), menu);
  if (!d.enabled) return null;
  const drag = (e: PointerEvent<HTMLElement>) => {
    const el = root.current!,
      desk = el.closest<HTMLElement>(".desk");
    if (!desk || d.locked || e.button !== 0) return;
    const k = desk.getBoundingClientRect().width / desk.offsetWidth;
    const x = e.clientX,
      y = e.clientY,
      left = el.offsetLeft,
      top = el.offsetTop;
    const node = e.currentTarget;
    node.setPointerCapture(e.pointerId);
    const move = (p: globalThis.PointerEvent) =>
      setPos({
        x: Math.max(0, Math.min(1920 - el.offsetWidth, left + (p.clientX - x) / k)),
        y: Math.max(0, Math.min(1032 - el.offsetHeight, top + (p.clientY - y) / k)),
      });
    const end = () => {
      node.removeEventListener("pointermove", move);
      node.removeEventListener("lostpointercapture", end);
    };
    node.addEventListener("pointermove", move);
    node.addEventListener("lostpointercapture", end);
  };
  return (
    <div
      className="w-desktop-monitor"
      ref={root}
      data-grouping={d.grouping}
      style={{
        ...(pos ? { left: pos.x, top: pos.y, right: "auto" } : {}),
        opacity: d.opacity / 100,
        pointerEvents: d.through ? "none" : undefined,
        zIndex: d.topmost ? 4 : 0,
      }}
    >
      <div
        onPointerDown={drag}
        onDoubleClick={() => store.set({ command: { action: "show", at: Date.now() } })}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu(!menu);
        }}
      >
        <MonitorFace kind="desktop" tick={tick} style={d.layout === "横条" ? d.layout : d.style} />
      </div>
      {menu && (
        <div className="w-popup">
          <Button
            onClick={() => {
              store.set({ command: { action: "show", at: Date.now() } });
              setMenu(false);
            }}
          >
            打开 Argus
          </Button>
          {["列表", "曲线", "仪表", "横条"].map((style) => (
            <Button
              key={style}
              onClick={() => {
                setMonitor("desktop", style === "横条" ? { layout: "横条" } : { layout: "面板", style });
                setMenu(false);
              }}
            >
              {style}
            </Button>
          ))}
          <Button onClick={() => setMonitor("desktop", { locked: !d.locked })} pressed={d.locked}>
            锁定位置
          </Button>
          <Button onClick={() => setMonitor("desktop", { topmost: !d.topmost })} pressed={d.topmost}>
            置顶显示
          </Button>
          <Button
            onClick={() => {
              setMenu(false);
              widgetSettings("桌面监控");
            }}
          >
            组件设置…
          </Button>
          <Button
            onClick={() => {
              setMenu(false);
              setMonitor("desktop", { enabled: false });
            }}
          >
            关闭桌面监控
          </Button>
        </div>
      )}
    </div>
  );
}
