"use client";
import { useSiteCopy } from "@/components/site/LocaleProvider";
import { dateLocale, type Language } from "@/lib/languages";
import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { sampleAt } from "../replica/model";
const resident = () => import("../replica/Resident");
const DesktopMonitor = lazy(() => resident().then((m) => ({ default: m.DesktopMonitor })));
const TaskMonitor = lazy(() => resident().then((m) => ({ default: m.TaskMonitor })));
const TrayGlyph = lazy(() => resident().then((m) => ({ default: m.TrayGlyph })));
const TrayPanel = lazy(() => resident().then((m) => ({ default: m.TrayPanel })));

import { store, useShared } from "../replica/store";

// The Windows 11 taskbar under the demo window: widgets on the left, Start and
// the pinned apps in the middle, the notification area and the clock on the
// right. Argus's tray icon sits among the hidden icons, as a newly installed
// app's does; its tooltip is the app's own (locales/*/native.json).
const words = {
  zh: {
    weather: "晴",
    start: "开始",
    search: "搜索",
    tasks: "任务视图",
    files: "文件资源管理器",
    browser: "浏览器",
    terminal: "终端",
    hidden: "显示隐藏的图标",
    memory: "内存",
  },
  en: {
    weather: "Sunny",
    start: "Start",
    search: "Search",
    tasks: "Task view",
    files: "File Explorer",
    browser: "Browser",
    terminal: "Terminal",
    hidden: "Show hidden icons",
    memory: "Memory",
  },
};

const glyph = (d: string) => (
  <svg
    width="20"
    height="20"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.5"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d={d} />
  </svg>
);

export default function Taskbar({ lang = "zh" }: { lang?: Language }) {
  const w = useSiteCopy(words);
  const [now, setNow] = useState<Date | null>(null);
  const [tick, setTick] = useState(0);
  const [hidden, setHidden] = useState(false);
  const { widgets, paused } = useShared();
  const [fly, setFly] = useState<"card" | "panel" | null>(null);
  const delay = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const cancel = () => clearTimeout(delay.current);
  const close = () => {
    cancel();
    setHidden(false);
    setFly(null);
  };
  useEffect(() => () => clearTimeout(delay.current), []);
  const taskbarWasActive = useRef<boolean | null>(null);
  const area = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const update = () => {
      setNow(new Date());
      if (!paused) setTick((t) => t + 1);
    };
    update();
    const id = setInterval(update, 1000);
    return () => clearInterval(id);
  }, [paused]);
  // The hidden icons close on a press elsewhere or Escape, as the flyout does.
  useEffect(() => {
    if (!hidden) return;
    const press = (event: PointerEvent) => {
      if (!(event.target instanceof Node) || !area.current?.contains(event.target)) close();
    };
    const key = (event: KeyboardEvent) => event.key === "Escape" && close();
    document.addEventListener("pointerdown", press);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("pointerdown", press);
      document.removeEventListener("keydown", key);
    };
  }, [hidden]);
  const s = sampleAt(tick);
  const tip = `Argus\nCPU ${Math.round(s.cpu)}%  ${w.memory} ${Math.round(s.mem)}%`;
  const app = () => area.current?.closest(".stage")?.querySelector(".argus-app");
  const send = (action: "show" | "minimize") => store.set({ command: { action, at: Date.now() } });
  return (
    <>
      <Suspense fallback={null}>{widgets.desktop.enabled && <DesktopMonitor tick={tick} />}</Suspense>
      <div className="taskbar">
        <span className="tb-weather" aria-hidden="true">
          <svg width="22" height="22" viewBox="0 0 24 24" aria-hidden="true">
            <circle cx="12" cy="12" r="5" fill="#f6b73c" />
            <path
              d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4"
              stroke="#f6b73c"
              strokeWidth="1.6"
              strokeLinecap="round"
            />
          </svg>
          <span>
            18°C
            <small>{w.weather}</small>
          </span>
        </span>
        <span className="tb-apps">
          <span className="tb tb-start" title={w.start}>
            <svg width="22" height="22" viewBox="0 0 24 24" aria-hidden="true">
              <path
                d="M2 2h9.5v9.5H2zM12.5 2H22v9.5h-9.5zM2 12.5h9.5V22H2zM12.5 12.5H22V22h-9.5z"
                fill="url(#tb-start)"
              />
              <defs>
                <linearGradient id="tb-start" x1="0" y1="0" x2="1" y2="1">
                  <stop offset="0" stopColor="#3ccbf4" />
                  <stop offset="1" stopColor="#0f6cbd" />
                </linearGradient>
              </defs>
            </svg>
          </span>
          <span className="tb" title={w.search}>
            {glyph("M10.5 17a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13Zm4.6-1.9L20 20")}
          </span>
          <span className="tb" title={w.tasks}>
            {glyph("M4 6.5h11v9H4zM9 4h11v9")}
          </span>
          <span className="tb is-running" title={w.files}>
            <i className="tb-folder" />
          </span>
          <span className="tb" title={w.browser}>
            <i className="tb-browser" />
          </span>
          <span className="tb" title={w.terminal}>
            <i className="tb-terminal" />
          </span>
          <button
            type="button"
            className="tb tb-argus is-running"
            data-argus-toggle
            onPointerDown={() => {
              taskbarWasActive.current = app()?.getAttribute("data-active") === "true";
            }}
            title="Argus"
            onClick={(e) => {
              const active = e.detail > 0 ? taskbarWasActive.current : app()?.getAttribute("data-active") === "true";
              taskbarWasActive.current = null;
              send(active ? "minimize" : "show");
            }}
          >
            <img src="/assets/argus.svg" width="24" height="24" alt="Argus" />
          </button>
        </span>
        <Suspense fallback={null}>
          {widgets.taskbar.enabled && (
            <TaskMonitor tick={tick} toggle={() => send(app()?.classList.contains("is-tray") ? "show" : "minimize")} />
          )}
        </Suspense>
        <div className="tb-tray" ref={area}>
          <button
            type="button"
            className={"tb-small" + (hidden ? " is-open" : "")}
            aria-label={w.hidden}
            title={w.hidden}
            aria-expanded={hidden}
            onClick={() => (hidden ? close() : setHidden(true))}
          >
            {glyph("m7 14 5-5 5 5")}
          </button>
          {hidden && (
            <div
              className="tb-flyout"
              onPointerLeave={() => {
                cancel();
                if (fly === "card") setFly(null);
              }}
            >
              <Suspense fallback={null}>
                {fly && <TrayPanel tick={tick} card={fly === "card"} close={close} />}
              </Suspense>
              <button
                type="button"
                className="tb-small"
                title={widgets.card ? undefined : tip}
                onPointerEnter={() => {
                  cancel();
                  if (widgets.card && !fly) delay.current = setTimeout(() => setFly("card"), 450);
                }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  cancel();
                  setFly(fly === "panel" ? null : "panel");
                }}
                aria-label={tip.replace("\n", " · ")}
                onClick={() => {
                  close();
                  send(app()?.classList.contains("is-tray") ? "show" : "minimize");
                }}
              >
                <Suspense fallback={<img src="/assets/argus.svg" alt="" width="18" height="18" />}>
                  <TrayGlyph tick={tick} style={widgets.icon} />
                </Suspense>
              </button>
            </div>
          )}
          <span className="tb-status" aria-hidden="true">
            {glyph("M2.5 9a14 14 0 0 1 19 0M5.5 12.5a9.5 9.5 0 0 1 13 0M8.5 16a5 5 0 0 1 7 0M12 19.5h.01")}
            {glyph("M4 9.5h3.5L12 5.5v13l-4.5-4H4zM15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11")}
          </span>
          <span className="tb-clock figures" aria-hidden="true">
            <span>{now ? now.toLocaleTimeString(dateLocale(lang), { hour: "2-digit", minute: "2-digit" }) : " "}</span>
            <span>{now ? now.toLocaleDateString(dateLocale(lang)) : " "}</span>
          </span>
          <span className="tb-small tb-bell" aria-hidden="true">
            {glyph("M6 16V11a6 6 0 1 1 12 0v5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0")}
          </span>
          <span className="tb-peek" aria-hidden="true" />
        </div>
      </div>
    </>
  );
}
