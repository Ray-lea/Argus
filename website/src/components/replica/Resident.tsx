/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useEffect, useRef, useState, type PointerEvent } from "react";
import { Icon, ProcessIcon } from "./icons";
import { appVersion, machineTotals, processes, readings, sampleAt, rate } from "./model";
import { byMetric } from "./metrics";
import { store, useShared, setWidgets, setMonitor, type Request } from "./store";
import { MiniCurve, MonitorFace, Styles, Readings } from "./MonitorReadings";
import { Button } from "./widgets";

// The tray icon's looks at 20 px (shell/tray_icon.rs): the window mark with the
// recent loads cut out of it, the hundred dots with an A, or the figure over a bar.
export function TrayGlyph({ tick, style }: { tick: number; style: string }) {
  const load = Math.min(100, Math.round(sampleAt(tick).cpu));
  const high = load >= 80;
  if (style === "应用图标") return <img src="/assets/argus.svg" width="20" height="20" alt="" />;
  if (style === "数字")
    return (
      <svg className="w-tray-glyph" width="20" height="20" viewBox="0 0 20 20" aria-hidden="true">
        <text x="10" y="13.4" textAnchor="middle" fontSize={load > 99 ? 10 : 13} fontWeight="600" fill={high ? "var(--tg-high)" : "currentColor"}>
          {load}
        </text>
        <rect x="1" y="17" width="18" height="2" rx="1" fill="currentColor" opacity="0.22" />
        <rect x="1" y="17" width={(18 * load) / 100} height="2" rx="1" fill={high ? "var(--tg-high)" : "var(--tg-load)"} />
      </svg>
    );
  if (style === "占比")
    return (
      <svg className="w-tray-glyph" width="20" height="20" viewBox="0 0 20 20" aria-hidden="true">
        {Array.from({ length: 25 }, (_, i) => {
          const column = i % 5,
            row = Math.floor(i / 5),
            rank = (4 - row) * 5 + column,
            letter = row === 0 ? column === 2 : row === 1 ? column === 1 || column === 3 : row === 2 ? column >= 1 && column <= 3 : column === 0 || column === 4;
          const on = rank < Math.round((load / 100) * 25);
          return (
            <rect
              key={i}
              x={1 + column * 4}
              y={1 + row * 4}
              width="2"
              height="2"
              rx=".5"
              fill={on ? (high ? "var(--tg-high)" : "var(--tg-accent)") : "currentColor"}
              opacity={on ? 1 : letter ? 0.42 : 0.2}
            />
          );
        })}
      </svg>
    );
  // 走势: five columns of three dots, lit from the bottom by the last five loads.
  const lit = (t: number) => Math.min(3, Math.max(1, Math.round((Math.min(100, sampleAt(t).cpu) / 100) * 3)));
  const dots = Array.from({ length: 5 }, (_, column) => ({ column, rows: lit(tick - 4 + column) }));
  const id = "tg" + tick;
  return (
    <svg className="w-tray-glyph" width="20" height="20" viewBox="0 0 20 20" aria-hidden="true">
      <mask id={id}>
        <rect x="1" y="1" width="18" height="18" rx="4.5" fill="white" />
        {dots.flatMap(({ column, rows }) =>
          Array.from({ length: rows }, (_, k) => (
            <rect key={column + "-" + k} x={3 + column * 3} y={9 + (2 - k) * 3} width="2" height="2" rx=".5" fill="black" />
          )),
        )}
        {[0, 1, 2].map((n) => (
          <rect key={"c" + n} x={3 + n * 3} y="4" width="2" height="2" rx=".5" fill="black" />
        ))}
      </mask>
      <rect x="1" y="1" width="18" height="18" rx="4.5" fill="currentColor" mask={`url(#${id})`} />
      {Array.from({ length: dots[4].rows }, (_, k) => (
        <rect key={k} x="15" y={9 + (2 - k) * 3} width="2" height="2" rx=".5" fill={high ? "var(--tg-high)" : "var(--tg-accent)"} />
      ))}
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
  const totals = machineTotals(tick);
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
    // The busiest GPU, and every adapter's rates (tray.rs).
    ["gpu", "gpu", "GPU", "gpu", totals.gpu, Math.round(totals.gpu) + "%", ""],
    ["net", "wifi", "网络", "network", 0, "↓ " + rate(totals.rx), "↑ " + rate(totals.tx)],
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
        <button role="menuitem" onClick={() => command("show")}>
          显示 Argus
        </button>
        <hr />
        <button role="menuitem" onClick={() => command("exit")}>
          退出
        </button>
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
          <div>
            <h4>样式</h4>
            <Styles kind="taskbar" tick={tick} />
            <h4>显示的读数</h4>
            <Readings kind="taskbar" compact />
          </div>
          <footer>
            <button
              onClick={() => {
                setPanel(false);
                widgetSettings("任务栏监控");
              }}
            >
              <Icon name="settings" size={15} />
              组件设置…
            </button>
            <button
              onClick={() => {
                setPanel(false);
                setMonitor("taskbar", { enabled: false });
              }}
            >
              <Icon name="power" size={15} />
              关闭任务栏监控
            </button>
          </footer>
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
        // The monitor's menu (shell/desk.rs `menu`): open, the look in a submenu, two checks, settings and close.
        <div className="w-popup w-desk-menu" role="menu">
          <button
            role="menuitem"
            onClick={() => {
              store.set({ command: { action: "show", at: Date.now() } });
              setMenu(false);
            }}
          >
            打开 Argus
          </button>
          <hr />
          <div className="w-desk-sub">
            <button role="menuitem" aria-haspopup="menu">
              显示方式
              <Icon name="chevronRight" size={14} />
            </button>
            <div className="w-popup w-desk-menu" role="menu">
              {(
                [
                  ["列表", "列表面板"],
                  ["曲线", "曲线面板"],
                  ["仪表", "仪表面板"],
                  ["-", ""],
                  ["横条", "横条"],
                ] as const
              ).map(([style, name]) =>
                style === "-" ? (
                  <hr key="-" />
                ) : (
                  <button
                    key={style}
                    role="menuitemradio"
                    aria-checked={style === "横条" ? d.layout === "横条" : d.layout === "面板" && d.style === style}
                    onClick={() => {
                      setMonitor("desktop", style === "横条" ? { layout: "横条" } : { layout: "面板", style });
                      setMenu(false);
                    }}
                  >
                    <span className="w-mark" />
                    {name}
                  </button>
                ),
              )}
            </div>
          </div>
          <button role="menuitemcheckbox" aria-checked={d.topmost} onClick={() => setMonitor("desktop", { topmost: !d.topmost })}>
            <span className="w-mark">{d.topmost && <Icon name="check" size={14} />}</span>
            置顶显示
          </button>
          <button role="menuitemcheckbox" aria-checked={d.locked} onClick={() => setMonitor("desktop", { locked: !d.locked })}>
            <span className="w-mark">{d.locked && <Icon name="check" size={14} />}</span>
            锁定位置
          </button>
          <hr />
          <button
            role="menuitem"
            onClick={() => {
              setMenu(false);
              widgetSettings("桌面监控");
            }}
          >
            组件设置…
          </button>
          <button
            role="menuitem"
            onClick={() => {
              setMenu(false);
              setMonitor("desktop", { enabled: false });
            }}
          >
            关闭桌面监控
          </button>
        </div>
      )}
    </div>
  );
}
