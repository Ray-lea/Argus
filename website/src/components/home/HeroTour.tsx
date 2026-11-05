/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { setLanguage, translate, type Language } from "../replica/i18n";
import { store, useShared } from "../replica/store";

// A guided demo of the window above it, in chapters the visitor can pick: a
// cursor walks through each page, its menus and dialogs, then the tray, until
// the visitor moves the mouse over the window, clicks or presses a key.
type Find = (app: HTMLElement, stage: HTMLElement) => Element | null | undefined;
interface Step {
  label: string;
  find?: Find;
  // What the cursor does there: a click by default.
  type?: string;
  menu?: boolean;
  at?: number;
  sweep?: [number, number];
  run?: () => void;
  hold?: number;
}
interface Chapter {
  label: string;
  steps: Step[];
}
// Targets are found by their text as the page shows it, in its language.
const byText = (root: ParentNode, selector: string, text: string) =>
  [...root.querySelectorAll(selector)].find((el) => el.textContent?.trim().startsWith(translate(text)));
const labelled = (selector: string, label: string) => `${selector}[aria-label="${translate(label)}"]`;
const nav = (id: string, text: string): Find => (app) =>
  app.querySelector(`[data-tour="nav-${id}"]`) ?? byText(app, ".a-nav button", text);
const tour = (id: string): Find => (app) => app.querySelector(`[data-tour="${id}"]`);
const titled = (title: string): Find => (app) => app.querySelector(`button[title="${translate(title)}"]`);
const segment = (text: string): Find => (app) => byText(app, ".a-window button[aria-checked]", text);
let startupItem = "";

const chapters: Chapter[] = [
  {
    label: "进程",
    steps: [
      { label: "打开进程页", find: nav("processes", "进程") },
      { label: "按 CPU 占用排序", find: tour("col-cpu") },
      { label: "选中占用最高的进程，查看详细信息", find: (a) => a.querySelector(".a-tbody .a-tr[data-key]") },
      { label: "右键菜单：结束进程树、设置优先级、效率模式与规则", find: (a) => a.querySelector(".a-tbody .a-tr.is-selected, .a-tbody .a-tr[data-key]"), menu: true, hold: 2200 },
      { label: "为该进程开启效率模式", find: (a) => byText(a, ".a-menu [role^=menuitem]", "效率模式") },
      { label: "切换到进程树，按父子关系查看", find: tour("view-tree") },
      { label: "回到按应用分组", find: tour("view-apps") },
      { label: "按名称排序", find: tour("col-name") },
    ],
  },
  {
    label: "性能",
    steps: [
      { label: "打开性能页", find: nav("performance", "性能") },
      { label: "CPU：利用率、频率、温度与功耗", find: tour("device-cpu") },
      { label: "悬停曲线，查看任一时刻的读数", find: (a) => a.querySelector(".a-chart .curve.is-readout"), sweep: [0.92, 0.3], hold: 2600 },
      { label: "每个逻辑处理器一张曲线，区分性能核与能效核", find: tour("graph-logical") },
      { label: "查看内存的使用情况", find: tour("device-memory") },
      { label: "查看磁盘的读写速率与活动时间", find: tour("device-disk") },
      { label: "查看显卡的引擎占用与专用显存", find: tour("device-gpu") },
    ],
  },
  {
    label: "连接",
    steps: [
      { label: "打开连接页", find: nav("connections", "连接") },
      { label: "按端口查找占用它的进程", find: (a) => a.querySelector('[data-tour="search"]') ?? a.querySelector(".a-search input"), type: "443" },
      { label: "选中连接，查看本地与远程端点", find: (a) => a.querySelector(".a-connections .a-tr") },
    ],
  },
  {
    label: "服务",
    steps: [
      { label: "打开服务页", find: nav("services", "服务") },
      {
        label: "选中正在运行的服务",
        find: (a) => [...a.querySelectorAll(".a-services .a-tr")].find((row) => row.textContent?.includes(translate("正在运行"))),
      },
      { label: "重新启动服务，状态随即更新", find: titled("重新启动服务"), hold: 2400 },
    ],
  },
  {
    label: "启动项",
    steps: [
      { label: "打开启动页", find: nav("startup", "启动") },
      {
        label: "禁用登录时启动的项目，下次登录时生效",
        find: (a) => {
          const prefix = translate("禁用 ");
          const toggle = a.querySelector<HTMLElement>(`.a-startup button[aria-label^="${prefix}"]`);
          startupItem = toggle?.getAttribute("aria-label")?.slice(prefix.length) ?? "";
          return toggle;
        },
      },
      {
        label: "重新启用该项目",
        find: (a) => a.querySelector(`.a-startup button[aria-label="${translate("启用 ")}${CSS.escape(startupItem)}"]`),
      },
    ],
  },
  {
    label: "历史",
    steps: [
      { label: "打开历史页，最近 7 天的记录保存在本机", find: nav("history", "历史") },
      { label: "切换时间范围", find: segment("1 天") },
      { label: "选择任一时刻，查看当时最忙的进程", find: (a) => a.querySelector(".a-history-plot"), at: 0.36, hold: 2600 },
    ],
  },
  {
    label: "工具",
    steps: [
      { label: "运行新任务，可选择以管理员身份运行", find: titled("运行新任务") },
      { label: "输入程序名称或路径", find: (a) => a.querySelector("#a-task-input"), type: "notepad.exe" },
      { label: "关闭对话框", find: (a) => byText(a, ".a-dialog.is-task .a-dialog-foot button", "取消") },
      { label: "查找占用文件的进程，也可将文件拖放到窗口", find: titled("查找文件占用（也可将文件拖放到窗口）") },
      {
        label: "列出占用该文件的进程，可直接结束",
        find: (a) => a.querySelector(".a-dialog.is-lock"),
        run: () => store.set({ command: { action: "lock", names: [translate("会议记录.txt")], at: Date.now() } }),
        hold: 2800,
      },
      { label: "关闭对话框", find: (a) => a.querySelector(".a-dialog.is-lock .a-dialog-foot [data-primary]") },
    ],
  },
  {
    label: "设置",
    steps: [
      { label: "打开设置页", find: nav("settings", "设置") },
      { label: "切换浅色主题", find: segment("浅色") },
      { label: "切换深色主题", find: segment("深色") },
      { label: "主题跟随系统", find: segment("跟随系统") },
      { label: "打开关于页，查看版本、当前权限与项目信息", find: nav("about", "关于"), hold: 2200 },
    ],
  },
  {
    label: "托盘",
    steps: [
      { label: "最大化窗口", find: (a) => a.querySelector(labelled(".a-caption ", "最大化")) },
      { label: "向下还原", find: (a) => a.querySelector(labelled(".a-caption ", "向下还原")) },
      { label: "关闭后留在托盘，采样、历史记录与规则继续运行", find: (a) => a.querySelector(labelled(".a-caption ", "关闭")), hold: 2400 },
      { label: "打开任务栏的隐藏图标", find: (_, s) => s.querySelector(".tb-tray > .tb-small") },
      { label: "单击托盘图标显示窗口，也可按 Ctrl+Alt+Esc", find: (_, s) => s.querySelector(".tb-flyout .tb-small") },
    ],
  },
];

const wait = (ms: number, signal: AbortSignal) =>
  new Promise<void>((resolve, reject) => {
    const id = setTimeout(resolve, ms);
    signal.addEventListener("abort", () => {
      clearTimeout(id);
      reject(new DOMException("stopped", "AbortError"));
    });
  });
function setValue(input: HTMLInputElement, value: string) {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}
// Runs step on every frame for ms, with progress from 0 to 1 eased in and out.
const glide = (ms: number, signal: AbortSignal, step: (progress: number) => void) =>
  new Promise<void>((resolve, reject) => {
    const begin = performance.now();
    let frame = requestAnimationFrame(function next(now) {
      const t = Math.min(1, (now - begin) / ms);
      step((1 - Math.cos(Math.PI * t)) / 2);
      if (t < 1) frame = requestAnimationFrame(next);
      else resolve();
    });
    signal.addEventListener("abort", () => {
      cancelAnimationFrame(frame);
      reject(new DOMException("stopped", "AbortError"));
    });
  });
// A point across the element, in the middle of its height.
const pointIn = (el: Element, fx: number) => {
  const r = el.getBoundingClientRect();
  return { clientX: r.left + r.width * fx, clientY: r.top + r.height / 2 };
};

export default function HeroTour({ lang = "zh" }: { lang?: Language }) {
  setLanguage(lang);
  const self = useRef<HTMLDivElement>(null);
  const cursor = useRef<HTMLDivElement>(null);
  const [mode, setMode] = useState<"waiting" | "playing" | "paused">("waiting");
  const [label, setLabel] = useState("");
  const [visible, setVisible] = useState(false);
  // The chapter on show, how far into it, and where the next run starts.
  const [chapter, setChapter] = useState(0);
  const [progress, setProgress] = useState(0);
  const [start, setStart] = useState({ chapter: 0, at: 0 });
  const touched = useRef(false);
  const [full, setFull] = useState(false);
  // The note on what the demo is: shown under the pointer or focus, and on a tap.
  const [note, setNote] = useState(false);
  const { request } = useShared();
  // A comparison row asked the window to show a page: the visitor has taken over.
  useEffect(() => {
    if (!request) return;
    touched.current = true;
    setMode((m) => (m === "playing" || m === "waiting" ? "paused" : m));
  }, [request]);

  // Full screen shows the desktop at its own 1920 × 1080 where the screen allows;
  // without the Fullscreen API (iPhone Safari) the stage covers the page instead.
  useEffect(() => {
    const stage = self.current?.closest<HTMLElement>(".stage");
    if (!stage) return;
    stage.classList.toggle("is-full", full);
    if (full && stage.requestFullscreen && !document.fullscreenElement) stage.requestFullscreen().catch(() => {});
    if (!full && document.fullscreenElement === stage) document.exitFullscreen().catch(() => {});
    const exit = () => document.fullscreenElement !== stage && setFull(false);
    const key = (event: KeyboardEvent) => event.key === "Escape" && setFull(false);
    document.addEventListener("fullscreenchange", exit);
    if (full) document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("fullscreenchange", exit);
      document.removeEventListener("keydown", key);
    };
  }, [full]);

  // Watch the stage, and hand over to the visitor at their first real input.
  useEffect(() => {
    const stage = self.current?.closest(".stage") as HTMLElement | null;
    if (!stage) return;
    const io = new IntersectionObserver(([e]) => setVisible(e.intersectionRatio > 0.45), {
      threshold: [0, 0.45, 0.8],
    });
    io.observe(stage);
    const take = (event: Event) => {
      if (!event.isTrusted) return;
      if (event.target instanceof Element && event.target.closest(".tour-bar")) return;
      touched.current = true;
      setMode((m) => (m === "playing" || m === "waiting" ? "paused" : m));
    };
    // Anything in the window takes over, and so does a click on the taskbar.
    const parts: [Element, string[]][] = [
      [stage.querySelector(".window") ?? stage, ["pointerdown", "wheel", "keydown", "dragenter", "pointermove"]],
      [stage.querySelector(".taskbar") ?? stage, ["pointerdown", "keydown"]],
    ];
    for (const [el, types] of parts) for (const type of types) el.addEventListener(type, take, true);
    return () => {
      io.disconnect();
      for (const [el, types] of parts) for (const type of types) el.removeEventListener(type, take, true);
    };
  }, []);

  // Start on its own once the window has hydrated and is in view, unless motion is reduced.
  useEffect(() => {
    if (mode !== "waiting" || !visible || touched.current) return;
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
      setMode("paused");
      return;
    }
    // The window hydrates with this component; it only has to be there.
    const id = setTimeout(() => {
      if (document.querySelector(".stage .argus-app")) setMode("playing");
    }, 1800);
    return () => clearTimeout(id);
  }, [mode, visible]);

  useEffect(() => {
    if (mode !== "playing" || !visible) return;
    const stage = self.current?.closest(".stage") as HTMLElement | null;
    const app = stage?.querySelector<HTMLElement>(".argus-app");
    const pointer = cursor.current;
    if (!stage || !app || !pointer) return;
    const control = new AbortController();
    const { signal } = control;
    const move = (x: number, y: number) => {
      const box = stage.getBoundingClientRect();
      pointer.style.transform = `translate(${x - box.left}px, ${y - box.top}px)`;
    };
    const press = () => {
      pointer.classList.remove("is-press");
      void pointer.offsetWidth;
      pointer.classList.add("is-press");
    };
    // Each chapter starts from a quiet window: no menu or dialog, not in the tray.
    const settle = async () => {
      if (app.querySelector(".a-menu, .a-dialog")) {
        app.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
        await wait(300, signal);
      }
      if (app.classList.contains("is-tray")) {
        store.set({ command: { action: "show", at: Date.now() } });
        await wait(500, signal);
      }
    };
    const act = async (step: Step, target: HTMLElement) => {
      const r = target.getBoundingClientRect();
      const fx = step.sweep?.[0] ?? step.at;
      const x = fx === undefined ? Math.min(r.left + Math.min(r.width / 2, 60), r.right - 6) : r.left + r.width * fx;
      const y = r.top + r.height / 2;
      move(x, y);
      await wait(760, signal);
      if (step.sweep) {
        // The pointer glides along the curve and the readout follows it on
        // every frame, as it follows a real pointer.
        const [from, to] = step.sweep;
        pointer.style.transition = "opacity 300ms ease";
        await glide(1700, signal, (progress) => {
          const at = pointIn(target, from + (to - from) * progress);
          target.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, ...at }));
          move(at.clientX, at.clientY);
        });
        await wait((step.hold ?? 1500) - 900, signal);
        pointer.style.transition = "";
        target.dispatchEvent(new PointerEvent("pointerout", { bubbles: true, relatedTarget: stage }));
        return;
      }
      press();
      if (step.run) step.run();
      else if (step.type !== undefined && target instanceof HTMLInputElement) {
        target.focus({ preventScroll: true });
        setValue(target, "");
        for (const ch of step.type) {
          await wait(130, signal);
          setValue(target, target.value + ch);
        }
      } else if (step.menu)
        target.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2, clientX: x, clientY: y }));
      else if (step.at !== undefined) target.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: x, clientY: y }));
      else target.click();
      await wait(step.hold ?? (step.type ? 1700 : 1500), signal);
    };
    (async () => {
      pointer.classList.add("is-on");
      for (let c = start.chapter; ; c = (c + 1) % chapters.length) {
        setChapter(c);
        setProgress(0);
        await settle();
        const { steps } = chapters[c];
        for (const [i, step] of steps.entries()) {
          // A step that leads off the page waits for what the step before it opened.
          let target = step.find?.(app, stage);
          if (!target && step.run) target = app;
          if (!(target instanceof HTMLElement)) continue;
          setLabel(step.label);
          setProgress((i + 1) / steps.length);
          await act(step, target);
        }
      }
    })().catch(() => {});
    return () => {
      control.abort();
      pointer.classList.remove("is-on");
      pointer.style.transition = "";
    };
  }, [mode, visible, start]);

  const playing = mode === "playing";
  return (
    <div className="tour" ref={self}>
      <div className="tour-cursor" ref={cursor} aria-hidden="true">
        <svg width="22" height="22" viewBox="0 0 22 22">
          <path d="M4 2.5 17.5 11l-6.2 1.4 3.6 6.6-2.6 1.4-3.6-6.6L4 18.2z" />
        </svg>
        <i />
      </div>
      <div className="tour-bar">
        <div className={"tour-chip" + (playing ? " is-playing" : "")}>
          <span className="tour-dot" aria-hidden="true" />
          <span className="tour-text" aria-live="polite">
            {playing ? label : mode === "paused" ? "演示已暂停，可直接操作窗口" : "界面演示 · 模拟数据"}
          </span>
          <span className="tour-note" data-open={note ? "" : undefined}>
            <button
              type="button"
              className="tour-note-button"
              aria-label="关于此演示"
              aria-describedby="tour-note"
              aria-expanded={note}
              onClick={() => setNote(!note)}
              onBlur={() => setNote(false)}
            >
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
                <circle cx="12" cy="12" r="9" />
                <path d="M12 11v5.5M12 7.6v.1" />
              </svg>
            </button>
            <span className="tour-note-tip" role="tooltip" id="tour-note">
              此演示在网页中重现 Argus 的界面与交互，仅供参考。受浏览器技术所限，动画、流畅度与细节可能不及实际程序，新版本的功能也可能尚未同步。请以实际程序为准。
            </span>
          </span>
          <button
            type="button"
            onClick={() => {
              touched.current = playing;
              if (!playing) setStart({ chapter, at: Date.now() });
              setMode(playing ? "paused" : "playing");
            }}
          >
            {playing ? "暂停" : "播放演示"}
          </button>
        </div>
        <div className="tour-chapters" role="group" aria-label="演示章节">
          {chapters.map((c, i) => (
            <button
              type="button"
              key={c.label}
              aria-pressed={i === chapter}
              data-i18n="nav"
              style={{ "--p": i === chapter ? (playing ? progress : 1) : 0 } as CSSProperties}
              onClick={() => {
                touched.current = false;
                setStart({ chapter: i, at: Date.now() });
                setMode("playing");
              }}
            >
              {c.label}
            </button>
          ))}
        </div>
        <button type="button" className="tour-full" aria-pressed={full} onClick={() => setFull(!full)}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d={full ? "M9 4v5H4m11-5v5h5M9 20v-5H4m11 5v-5h5" : "M4 9V4h5m6 0h5v5M4 15v5h5m6 0h5v-5"} />
          </svg>
          {full ? "退出全屏" : "全屏预览"}
        </button>
      </div>
    </div>
  );
}
