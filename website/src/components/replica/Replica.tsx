/** @jsxImportSource @/components/replica/i18n */
"use client";
import {
  useCallback,
  useEffect,
  useEffectEvent,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type DragEvent,
  type KeyboardEvent,
  type MouseEvent,
} from "react";
import {
  ColumnsDialog,
  columnSortValue,
  defaultPrefs,
  descendingFirst,
  headerMenu,
  moveBefore,
  setSelected as selectColumn,
  visibleColumns,
  type ColumnPrefs,
} from "./Columns";
import { advanceClock, sampleClock, type Motion } from "./Curve";
import { registerDictionary, setLanguage, type Dictionary, type Language } from "./i18n";
import FileLock, { inspect, type LockResult } from "./FileLock";
import { CaptionIcon, Icon, type UiIcon } from "./icons";
import {
  historyRanges,
  processes,
  readings,
  sampleAt,
  services as serviceList,
  span,
  startupItems,
  thousands,
  type Process,
  type Reading,
  type Service,
} from "./model";
import type { ConnectionFilter, HistoryRange, MenuEntry, ServiceFilter, ServiceState, ThemeChoice } from "./Pages";
import { buildRows, Details, ProcessTable, Vitals, type Device, type Row, type SortKey, type View } from "./Processes";
import { useShared, type Page } from "./store";
import { Button, localRect, scaleOf, Segments, Select, slideIn, useIsoLayoutEffect } from "./widgets";

// Only the processes page shows at first; the others' code loads off the
// first paint's path. React.lazy would hold a page that loads late behind a
// fallback for another 300 ms, so the modules are kept in state instead:
// loaded when the browser is idle, or at once when their page opens first.
const loadPages = () => import("./Pages");
const loadPerformance = () => import("./Performance");
function useLater<T>(load: () => Promise<T>, needed: boolean): T | null {
  const [loaded, setLoaded] = useState<T | null>(null);
  useEffect(() => {
    if (loaded) return;
    let live = true;
    const get = () => void load().then((m) => live && setLoaded(() => m));
    if (needed) {
      get();
      return () => {
        live = false;
      };
    }
    if (typeof requestIdleCallback === "function") {
      const id = requestIdleCallback(get, { timeout: 4000 });
      return () => {
        live = false;
        cancelIdleCallback(id);
      };
    }
    const id = setTimeout(get, 1500);
    return () => {
      live = false;
      clearTimeout(id);
    };
  }, [load, needed, loaded]);
  return loaded;
}

const pages: [Page, string, UiIcon][] = [
  ["processes", "进程", "processes"],
  ["performance", "性能", "pulse"],
  ["services", "服务", "services"],
  ["startup", "启动", "power"],
  ["connections", "连接", "globe"],
  ["history", "历史", "history"],
  ["settings", "设置", "settings"],
  ["about", "关于", "info"],
];
const critical = new Set([4, 148, 2864, 520, 812, 916, 904, 1004, 1068, 1088]);
const searchHint: Partial<Record<Page, string>> = {
  processes: "搜索名称、PID、路径或服务",
  services: "搜索服务名称、描述或 PID",
  startup: "搜索名称、发布者或命令",
  connections: "端口、地址或进程，例如 8080",
};
const priorities: [string, string][] = [
  ["realtime", "实时"],
  ["high", "高"],
  ["aboveNormal", "高于正常"],
  ["normal", "正常"],
  ["belowNormal", "低于正常"],
  ["idle", "低"],
];
interface MenuState {
  x: number;
  y: number;
  items: MenuEntry[];
  keyboard: boolean;
  id: number;
}
type Dialog =
  | { kind: "lock" }
  | { kind: "run" }
  | { kind: "end"; row: Row; tree: boolean }
  | { kind: "realtime"; row: Row }
  | { kind: "rule"; row: Row }
  | { kind: "columns" };
interface Toast {
  text: string;
  id: number;
  tone: "ok" | "info" | "error";
}

// The toolbar's compact levels (layout.rs `fit_toolbar`): 1 drops the trailing
// buttons' text, 2 the view labels, 3 moves the page controls to a row of their
// own, 4 the search box too.
function useToolbarFit(key: string, hasSearch: boolean, searchFit: number) {
  const bar = useRef<HTMLDivElement>(null);
  const start = useRef<HTMLDivElement>(null);
  const trailing = useRef<HTMLDivElement>(null);
  const [level, setLevel] = useState(0);
  const [search, setSearch] = useState<{ left: number; width: number } | null>(null);
  const need = useRef<number[]>([]);
  const reset = useRef(key);
  if (reset.current !== key) {
    // Another page, navigation or text starts over from the widest level.
    reset.current = key;
    need.current = [];
    if (level !== 0) setLevel(0);
  }
  const measure = useCallback(() => {
    const row = bar.current;
    const a = start.current;
    const b = trailing.current;
    if (!row || !a || !b) return;
    const box = localRect(row);
    const width = box.width + 102; // the caption buttons beside the row
    const trail = localRect(b);
    const left = localRect(a).right - box.left + 8;
    const right = trail.left - box.left - 8;
    let current = level;
    while (current > 0 && width + 0.5 >= (need.current[current - 1] ?? Infinity)) current--;
    if (current !== level) {
      setLevel(current);
      return;
    }
    const room = hasSearch && current < 4 ? searchFit + 16 : 8;
    // The start may not reach the search box (or the end controls), nor the
    // end controls pass the row's 4 px inset before the caption buttons.
    const overflow = Math.max(left - 8 + room - (right + 8), trail.right - (box.right - 4));
    if (overflow > 0.5 && current < 4) {
      need.current[current] = width + overflow;
      setLevel(current + 1);
      return;
    }
    if (hasSearch && current < 4) {
      const w = Math.max(0, Math.min(360, right - left));
      const centered = (box.width + 102 - w) / 2 - 1;
      setSearch((s) => {
        const next = { left: Math.round(Math.max(left, Math.min(right - w, centered))), width: Math.round(w) };
        return s && s.left === next.left && s.width === next.width ? s : next;
      });
    }
  }, [level, hasSearch, searchFit]);
  useIsoLayoutEffect(() => {
    measure();
  });
  useEffect(() => {
    const row = bar.current;
    if (!row) return;
    const observer = new ResizeObserver(() => measure());
    observer.observe(row);
    return () => observer.disconnect();
  }, [measure]);
  return { bar, start, trailing, level, search };
}

type DetailsMode = "open" | "auto" | "closed";
const DETAILS_MODES: DetailsMode[] = ["open", "auto", "closed"];
const DETAILS_MODE_NAMES: Record<DetailsMode, string> = {
  open: "始终显示",
  auto: "选中进程时显示",
  closed: "隐藏",
};
const nextDetails = (mode: DetailsMode): DetailsMode =>
  DETAILS_MODES[(DETAILS_MODES.indexOf(mode) + 1) % DETAILS_MODES.length];

export default function Replica({
  initialPage = "processes",
  lang = "zh",
  dictionary,
}: {
  initialPage?: Page;
  lang?: Language;
  dictionary?: Dictionary;
}) {
  // Text below is translated as it renders (./i18n).
  setLanguage(lang);
  if (dictionary) registerDictionary(dictionary);
  const root = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const shared = useShared();
  const [page, setPage] = useState<Page>(initialPage);
  const [dir, setDir] = useState(0);
  const [device, setDevice] = useState<Device>("cpu");
  const [view, setView] = useState<View>("apps");
  const [sort, setSort] = useState<SortKey>("name");
  const [descending, setDescending] = useState(false);
  // The process columns shown and their order (Columns.tsx).
  const [columnPrefs, setColumnPrefs] = useState<ColumnPrefs>(defaultPrefs);
  const shownColumns = useMemo(() => visibleColumns(columnPrefs), [columnPrefs]);
  const [queries, setQueries] = useState<Partial<Record<Page, string>>>({});
  const [serviceFilter, setServiceFilter] = useState<ServiceFilter>("all");
  const [connectionFilter, setConnectionFilter] = useState<ConnectionFilter>("all");
  const [historyRange, setHistoryRange] = useState<HistoryRange>("1h");
  const [historyAt, setHistoryAt] = useState<number | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [service, setService] = useState<string | null>(null);
  // When the details pane shows, as the app's toolbar button cycles it:
  // always, while a process is selected, or never.
  const [detailsMode, setDetailsMode] = useState<DetailsMode>("open");
  // The pane slides in when it is opened, not when the page is shown again.
  const [paneEnters, setPaneEnters] = useState(false);
  const [open, setOpen] = useState<Set<string>>(() => new Set());
  const [closed, setClosed] = useState<Set<string>>(() => new Set());
  const [hold, setHold] = useState(false);
  const order = useRef<string[]>([]);
  const [ended, setEnded] = useState<Set<number>>(() => new Set());
  const [efficiency, setEfficiency] = useState<Set<number>>(() => new Set());
  const [priority, setPriority] = useState<Map<number, string>>(() => new Map());
  const [serviceStates, setServiceStates] = useState<Map<string, ServiceState>>(
    () => new Map(serviceList.map((s) => [s.name, s.pid ? "running" : "stopped"])),
  );
  const [startup, setStartup] = useState(startupItems);
  const [interval, setIntervalMs] = useState(1000);
  const [paused, setPaused] = useState(false);
  const [ctrlHeld, setCtrlHeld] = useState(false);
  const [motion, setMotion] = useState<Motion>("smooth");
  const [theme, setTheme] = useState<ThemeChoice>("site");
  const [scale, setScale] = useState(100);
  const [fontSize, setFontSize] = useState(13);
  const [navLeft, setNavLeft] = useState(false);
  const [navOpen, setNavOpen] = useState(false);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [lock, setLock] = useState<LockResult | null>(null);
  const [checking, setChecking] = useState(false);
  const [elevated, setElevated] = useState(false);
  const [command, setCommand] = useState("");
  const [dragging, setDragging] = useState(false);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [toast, setToast] = useState<Toast | null>(null);
  const [tray, setTray] = useState<null | "tray" | "exited">(null);
  // The demo shows everything the app can, run as administrator.
  const [maximized, setMaximized] = useState(false);
  const [starts, setStarts] = useState<Map<string, Service["start"]>>(() => new Map());
  const services = useMemo(
    () => serviceList.map((s) => (starts.has(s.name) ? { ...s, start: starts.get(s.name)! } : s)),
    [starts],
  );
  const [onScreen, setOnScreen] = useState(true);
  const [tick, setTick] = useState(0);
  const later = useLater(loadPages, page !== "processes" && page !== "performance");
  const { Services, Startup, Connections, History, Settings, About } = later ?? {};
  const Performance = useLater(loadPerformance, page === "performance")?.default;
  const sampled = page === "processes" || page === "performance";
  const running = !paused && !(ctrlHeld && sampled) && onScreen && !tray;

  useEffect(() => {
    const el = root.current;
    if (!el) return;
    let seen = true,
      shown = document.visibilityState === "visible";
    const update = () => setOnScreen(seen && shown);
    const io = new IntersectionObserver(([entry]) => {
      seen = entry.isIntersecting;
      update();
    });
    io.observe(el);
    const vis = () => {
      shown = document.visibilityState === "visible";
      update();
    };
    document.addEventListener("visibilitychange", vis);
    return () => {
      io.disconnect();
      document.removeEventListener("visibilitychange", vis);
    };
  }, []);
  const sampledBefore = useRef(false);
  useEffect(() => {
    if (!running) return;
    // A sample at once when sampling resumes, so the curves move as soon as
    // the window comes back into view; while it is held they rest where the
    // last scroll ended. Not on the first start: the window has just been
    // drawn, and drawing it again at once doubled the page's longest task.
    const sample = () => {
      advanceClock(sampleClock, interval);
      setTick((t) => t + 1);
    };
    if (sampledBefore.current) sample();
    sampledBefore.current = true;
    const id = setInterval(sample, interval);
    return () => {
      clearInterval(id);
      sampleClock.rest = true;
    };
  }, [running, interval]);
  useEffect(() => {
    if (!toast) return;
    const id = setTimeout(() => setToast(null), 4200);
    return () => clearTimeout(id);
  }, [toast]);
  useEffect(() => {
    if (tray !== "tray") return;
    const key = (event: globalThis.KeyboardEvent) => {
      if (event.ctrlKey && event.altKey && event.key === "Escape") setTray(null);
    };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [tray]);
  // Holding Ctrl pauses the updates on the sampled pages until it is released.
  useEffect(() => {
    if (!ctrlHeld) return;
    const up = (event: globalThis.KeyboardEvent) => !event.ctrlKey && setCtrlHeld(false);
    const blur = () => setCtrlHeld(false);
    document.addEventListener("keyup", up);
    window.addEventListener("blur", blur);
    return () => {
      document.removeEventListener("keyup", up);
      window.removeEventListener("blur", blur);
    };
  }, [ctrlHeld]);

  // A new page moves in from the side of its tab (from below with the rail on the left).
  const content = useRef<HTMLElement>(null);
  useIsoLayoutEffect(() => {
    if (dir) slideIn(content.current, navLeft ? { y: dir * 24, duration: 300 } : { x: dir * 32, duration: 300 });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [page]);
  const go = useCallback(
    (next: Page) => {
      setMenu(null);
      setPaneEnters(false);
      setPage((current) => {
        if (current !== next) {
          const a = pages.findIndex((p) => p[0] === current);
          const b = pages.findIndex((p) => p[0] === next);
          setDir(Math.sign(b - a));
        }
        return next;
      });
    },
    [],
  );
  // Another section of the page can ask the window to show one of its pages.
  useEffect(() => {
    const request = shared.request;
    if (!request) return;
    go(request.page);
    if (request.view) {
      setView(request.view);
      setSort("name");
      setDescending(false);
    }
    setTray(null);
    root.current?.scrollIntoView({
      behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth",
      block: "center",
    });
  }, [shared.request, go]);
  // The taskbar shows the window again or minimizes it; the guided demo drops a file.
  const lookUp = useEffectEvent((names: string[]) => files(names, false));
  useEffect(() => {
    const command = shared.command;
    if (!command) return;
    if (command.action === "lock") lookUp(command.names);
    else if (command.action === "show") setTray(null);
    else setTray((current) => current ?? "tray");
  }, [shared.command]);
  const closeMenu = useCallback(() => setMenu(null), []);
  // The window keeps the keyboard when what had it goes away (another page,
  // a closed dialog or menu), as a desktop window does.
  const focused = useRef(false);
  useEffect(() => {
    const away = (event: Event) => {
      if (!(event.target instanceof Node) || !root.current?.contains(event.target)) focused.current = false;
    };
    document.addEventListener("pointerdown", away, true);
    return () => document.removeEventListener("pointerdown", away, true);
  }, []);
  useEffect(() => {
    if (focused.current && (document.activeElement === document.body || !document.activeElement))
      root.current?.focus({ preventScroll: true });
  });
  // A dialog takes the keyboard as it opens (its own field, if it has one, keeps it).
  const focusDialog = useCallback((el: HTMLElement | null) => {
    const dialog = el?.querySelector<HTMLElement>(".a-dialog");
    if (dialog && !dialog.contains(document.activeElement)) dialog.focus({ preventScroll: true });
  }, []);
  const say = (text: string, tone: Toast["tone"] = "ok") => setToast({ text, id: Date.now(), tone });
  const alive = useMemo(() => processes.filter((p) => !ended.has(p.pid)), [ended]);
  const byPid = useMemo(() => new Map(alive.map((p) => [p.pid, p])), [alive]);
  const now = useMemo(() => readings(tick, alive), [tick, alive]);
  const past = useMemo(() => {
    const out: Map<number, Reading>[] = [];
    for (let t = tick - span - 1; t <= tick; t++) out.push(t === tick ? now : readings(t, alive));
    return out;
  }, [tick, alive, now]);
  const history = useCallback(
    (pids: number[]) => ({
      cpu: past.map((m) => pids.reduce((a, pid) => a + (m.get(pid)?.cpu ?? 0), 0)),
      mem: past.map((m) => pids.reduce((a, pid) => a + (m.get(pid)?.mem ?? 0), 0)),
    }),
    [past],
  );

  const query = queries.processes ?? "";
  const cellContext = { byPid, tick, efficiency, priority };
  let rows = buildRows({
    view,
    sort,
    descending,
    query,
    list: alive,
    now,
    open,
    closed,
    sortValue: (row) => columnSortValue(sort, row, cellContext),
  });
  // While the pointer rests on the list the row order holds; values keep updating.
  if (hold && order.current.length) {
    const rank = new Map(order.current.map((key, i) => [key, i]));
    const known = rows.filter((r) => rank.has(r.key)).sort((a, b) => rank.get(a.key)! - rank.get(b.key)!);
    rows = [...known, ...rows.filter((r) => !rank.has(r.key))];
  } else order.current = rows.map((r) => r.key);
  const selectedRow = rows.find((r) => r.key === selected) ?? null;
  const detailsOpen = detailsMode === "open" || (detailsMode === "auto" && selectedRow !== null);
  // A row asked for its details: a hidden pane opens and stays open.
  const revealDetails = () => setDetailsMode((mode) => (mode === "closed" ? "open" : mode));
  const chooseDetails = (mode: DetailsMode) => {
    setPaneEnters(true);
    setDetailsMode(mode);
  };

  const toggleRow = (row: Row) => {
    const folding = row.kind === "group" || view === "tree";
    const set = folding ? closed : open;
    const next = new Set(set);
    if (next.has(row.key)) next.delete(row.key);
    else next.add(row.key);
    if (folding) setClosed(next);
    else setOpen(next);
  };
  const sortBy = (key: SortKey) => {
    if (key === sort) setDescending(!descending);
    else {
      setSort(key);
      setDescending(descendingFirst(key));
    }
  };
  // Hiding the column the list is sorted by sorts it by name, as the app does.
  useIsoLayoutEffect(() => {
    if (!columnPrefs.selected.includes(sort)) {
      setSort("name");
      setDescending(false);
    }
  }, [columnPrefs, sort]);
  const toggleColumn = (key: string) => setColumnPrefs((p) => selectColumn(p, key, !p.selected.includes(key)));
  const openHeaderMenu = (event: MouseEvent, column: string | null) =>
    showMenu(
      { x: event.clientX, y: event.clientY },
      headerMenu({
        column,
        prefs: columnPrefs,
        onSort: (key, desc) => {
          setSort(key);
          setDescending(desc);
        },
        onToggle: toggleColumn,
        onChoose: () => setDialog({ kind: "columns" }),
        onAutoWidth: () => setColumnPrefs((p) => ({ ...p, autoWidth: !p.autoWidth })),
        onReset: () => setColumnPrefs(defaultPrefs()),
      }),
    );
  const showPid = (pid: number) => {
    go("processes");
    setQueries((q) => ({ ...q, processes: "" }));
    setView("list");
    setClosed(new Set());
    setSelected("p:" + pid);
    revealDetails();
    setDialog(null);
    setTimeout(() => root.current?.querySelector(`[data-key="p:${pid}"]`)?.scrollIntoView({ block: "center" }), 60);
  };
  const descendants = (pid: number): number[] => {
    const kids = alive.filter((p) => p.ppid === pid && p.pid !== pid);
    return kids.flatMap((k) => [k.pid, ...descendants(k.pid)]);
  };
  const finish = (row: Row, tree: boolean) => {
    const pids = tree ? [row.pids[0], ...descendants(row.pids[0])] : row.pids;
    const procs = pids.map((pid) => byPid.get(pid)).filter(Boolean) as Process[];
    const name = row.label;
    setDialog(null);
    if (procs.some((p) => critical.has(p.pid))) {
      say(`${name} 是系统关键进程，结束该进程会导致系统崩溃，操作已阻止。`, "error");
      return;
    }
    if (pids.includes(16072)) {
      setTray("exited");
      return;
    }
    setEnded((e) => new Set([...e, ...pids]));
    if (selectedRow?.pids.some((pid) => pids.includes(pid))) setSelected(null);
    say(tree ? `已结束 ${pids.length} 个进程。` : pids.length > 1 ? `已结束 ${name} 的 ${pids.length} 个进程。` : `已结束 ${name}。`);
  };
  const end = (row: Row, confirm: boolean, tree = false) => {
    setMenu(null);
    if (confirm) setDialog({ kind: "end", row, tree });
    else finish(row, tree);
  };
  const rowFor = (proc: Process): Row => ({
    key: "p:" + proc.pid,
    kind: "unit",
    depth: 0,
    label: proc.name,
    exe: proc.exe,
    count: 1,
    pids: [proc.pid],
    glyph: proc.glyph,
    group: proc.group,
    expandable: false,
    expanded: false,
    values: now.get(proc.pid) ?? { cpu: 0, mem: 0, read: 0, write: 0, io: 0, gpu: 0, net: 0 },
  });
  const files = (names: string[], folder: boolean) => {
    setDialog({ kind: "lock" });
    setChecking(true);
    setTimeout(() => {
      setLock(inspect(names, folder, alive));
      setChecking(false);
    }, 520);
  };
  const drop = (event: DragEvent) => {
    event.preventDefault();
    setDragging(false);
    const items = [...event.dataTransfer.items];
    const folder = items.some((item) => item.webkitGetAsEntry?.()?.isDirectory);
    const names = [...event.dataTransfer.files].map((f) => f.name);
    if (names.length) files(names, folder);
  };
  const copy = (text: string, what: string) => {
    void navigator.clipboard?.writeText(text).catch(() => {});
    say(`已复制${what}。`);
  };
  const setPriorityOf = (row: Row, level: string) => {
    setPriority((m) => {
      const next = new Map(m);
      for (const pid of row.pids) next.set(pid, level);
      return next;
    });
  };
  const toggleEfficiency = (row: Row) => {
    const on = !row.pids.every((pid) => efficiency.has(pid));
    setEfficiency((set) => {
      const next = new Set(set);
      for (const pid of row.pids) on ? next.add(pid) : next.delete(pid);
      return next;
    });
    say(on ? `已为 ${row.label} 开启效率模式。` : `已为 ${row.label} 关闭效率模式。`);
  };

  // Menus open at the pointer toward the free side, as Windows menus do.
  const showMenu = (at: { x: number; y: number }, items: MenuEntry[], keyboard = false) => {
    const frame = root.current!.querySelector(".a-window")!;
    const box = frame.getBoundingClientRect(),
      k = scaleOf(frame);
    setMenu({ x: (at.x - box.left) / k, y: (at.y - box.top) / k, items, keyboard, id: Date.now() });
  };
  const processMenu = (row: Row): MenuEntry[] => {
    const group = row.count > 1;
    const proc = byPid.get(row.pids[0]);
    const current = new Set(row.pids.map((pid) => priority.get(pid) ?? "正常"));
    const shared = current.size === 1 ? [...current][0] : null;
    const efficient = row.pids.every((pid) => efficiency.has(pid));
    return [
      { label: group ? `结束全部 ${row.count} 个进程` : "结束进程", icon: "end", hint: "Del", danger: true, run: () => end(row, true) },
      ...(group ? [] : [{ label: "结束进程树", icon: "branchEnd" as UiIcon, run: () => end(row, true, true) }]),
      ...(proc?.exe === "explorer.exe"
        ? [{ label: "重新启动", icon: "restart" as UiIcon, run: () => say("演示窗口不会重新启动资源管理器。", "info") }]
        : []),
      "-",
      {
        label: "设置优先级",
        icon: "gauge",
        children: priorities.map(([id, label]) => ({
          label,
          checked: shared === label,
          run: () => {
            if (id === "realtime") setDialog({ kind: "realtime", row });
            else {
              setPriorityOf(row, label);
              say(`${row.label} 的优先级已设为“${label}”。`);
            }
          },
        })),
      },
      { label: "效率模式", icon: "leaf", checked: efficient, run: () => toggleEfficiency(row) },
      { label: "添加规则", run: () => setDialog({ kind: "rule", row }) },
      "-",
      ...(row.expandable ? [{ label: row.expanded ? "折叠" : "展开", run: () => toggleRow(row) }] : []),
      { label: "打开文件位置", icon: "folder", run: () => say(`演示窗口不会打开 ${proc?.path ?? "该位置"}。`, "info") },
      { label: "复制路径", icon: "copy", run: () => copy(proc?.path ?? "", "路径") },
      group
        ? { label: "复制名称", run: () => copy(proc?.exe ?? row.label, "名称") }
        : { label: "复制 PID", run: () => copy(String(row.pids[0]), " PID") },
      "-",
      {
        label: "详细信息",
        icon: "info",
        hint: "Enter",
        run: () => {
          setSelected(row.key);
          revealDetails();
        },
      },
    ];
  };
  const openRowMenu = (event: MouseEvent | KeyboardEvent, row: Row) => {
    if ("clientX" in event) showMenu({ x: event.clientX, y: event.clientY }, processMenu(row));
    else {
      const line = root.current!.querySelector(`[data-key="${CSS.escape(row.key)}"]`)!;
      const el = line.getBoundingClientRect();
      showMenu({ x: el.left + 40 * scaleOf(line), y: el.bottom }, processMenu(row), true);
    }
  };
  const control = (name: string, action: "start" | "stop" | "restart") => {
    const target = action === "stop" ? "stopPending" : "startPending";
    setServiceStates((m) => new Map(m).set(name, target));
    setTimeout(() => {
      setServiceStates((m) => new Map(m).set(name, action === "stop" ? "stopped" : "running"));
      const display = serviceList.find((x) => x.name === name)?.display;
      say(action === "start" ? `已启动“${display}”。` : action === "stop" ? `已停止“${display}”。` : `已重新启动“${display}”。`);
    }, 800);
  };
  const serviceMenu = (event: MouseEvent, s: Service) => {
    const state = serviceStates.get(s.name);
    const pending = !!state?.endsWith("Pending");
    showMenu({ x: event.clientX, y: event.clientY }, [
      { label: "启动", icon: "play", disabled: state === "running" || pending, run: () => control(s.name, "start") },
      { label: "停止", icon: "stop", disabled: state !== "running" || pending, run: () => control(s.name, "stop") },
      { label: "重新启动", icon: "restart", disabled: state !== "running" || pending, run: () => control(s.name, "restart") },
      "-",
      ...(["auto", "autoDelayed", "manual", "disabled"] as const).map((type) => ({
        label: `启动类型：${{ auto: "自动", autoDelayed: "自动（延迟启动）", manual: "手动", disabled: "禁用" }[type]}`,
        checked: s.start === type,
        run: () => {
          setStarts((m) => new Map(m).set(s.name, type));
          say(`“${s.display}”的启动类型已设为“${{ auto: "自动", autoDelayed: "自动（延迟启动）", manual: "手动", disabled: "禁用" }[type]}”。`);
        },
      })),
      "-",
      { label: "转到进程", icon: "goTo", disabled: !s.pid || state !== "running", run: () => s.pid && showPid(s.pid) },
      { label: "打开文件位置", icon: "folder" },
      { label: "复制服务名称", icon: "copy", run: () => copy(s.name, "服务名称") },
      "-",
      { label: "置顶", icon: "pin" },
      { label: "添加备注", icon: "note" },
    ]);
  };

  const keys = (event: KeyboardEvent<HTMLDivElement>) => {
    const target = event.target as HTMLElement;
    const typing = target.closest("input, textarea, [role=combobox], [role=menu]");
    if (event.key === "Control" && !event.repeat && sampled && !dialog) setCtrlHeld(true);
    if (event.ctrlKey && !event.altKey && /^[1-8]$/.test(event.key)) {
      event.preventDefault();
      go(pages[Number(event.key) - 1][0]);
    } else if (event.ctrlKey && event.key.toLowerCase() === "f" && searchHint[page]) {
      event.preventDefault();
      search.current?.focus();
      search.current?.select();
    } else if (event.key === "/" && !typing && searchHint[page] && !dialog) {
      event.preventDefault();
      search.current?.focus();
    } else if (
      page === "processes" &&
      !typing &&
      !dialog &&
      !menu &&
      event.key.length === 1 &&
      /[\p{L}\p{N}]/u.test(event.key) &&
      !event.ctrlKey &&
      !event.altKey &&
      !event.metaKey
    ) {
      // On the process page, typing a letter or digit starts the search.
      event.preventDefault();
      setQueries((q) => ({ ...q, processes: (q.processes ?? "") + event.key }));
      search.current?.focus();
    } else if (event.key === "Escape") {
      if (menu) setMenu(null);
      else if (dialog) setDialog(null);
      else if (target === search.current && (queries[page] ?? "")) setQueries((q) => ({ ...q, [page]: "" }));
      else if (page === "processes" && selected) setSelected(null);
    }
  };
  const s = sampleAt(tick);
  const threads = alive.reduce((a, p) => a + p.threads, 1180);
  const handles = alive.reduce((a, p) => a + p.handles, 52400);
  const accent = shared.accent
    ? ({ "--a-accent-l": shared.accent.light, "--a-accent-d": shared.accent.dark } as Record<string, string>)
    : undefined;
  const mode = theme === "site" ? undefined : theme;
  const hasSearch = !!searchHint[page];
  // The narrowest the search box gets: its placeholder and the room around it.
  const searchFit = useMemo(() => {
    const hint = searchHint[page] ?? "";
    const canvas = typeof document === "undefined" ? null : document.createElement("canvas").getContext("2d");
    if (!canvas) return hint.length * 12 + 48;
    canvas.font = '13px "Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI", sans-serif';
    return Math.ceil(canvas.measureText(hint).width) + 48;
  }, [page]);
  const fit = useToolbarFit(`${page}|${navLeft}|${navOpen}`, hasSearch, searchFit);
  const level = fit.level;
  const wide = level === 0;
  const serviceState = service ? serviceStates.get(service) : undefined;

  const controls =
    page === "processes" ? (
      <Segments
        key="processes"
        label="视图"
        value={view}
        onChange={setView}
        iconsOnly={level === 2}
        items={[
          { value: "apps", label: "按应用", icon: "apps", title: "按应用：同一程序的多个进程合并为一行", tour: "view-apps" },
          { value: "tree", label: "进程树", icon: "tree", title: "进程树：按父子关系显示", tour: "view-tree" },
          { value: "list", label: "列表", icon: "list", title: "列表：所有进程平铺显示", tour: "view-list" },
        ]}
      />
    ) : page === "services" ? (
      <Segments
        key="services"
        label="筛选"
        value={serviceFilter}
        onChange={setServiceFilter}
        items={[
          { value: "all", label: "全部" },
          { value: "running", label: "正在运行" },
          { value: "stopped", label: "已停止" },
        ]}
      />
    ) : page === "connections" ? (
      <Segments
        key="connections"
        label="筛选"
        value={connectionFilter}
        onChange={setConnectionFilter}
        items={[
          { value: "all", label: "全部", title: "所有 TCP 和 UDP 端点" },
          { value: "listen", label: "监听", title: "正在等待连接的 TCP 端口" },
          { value: "established", label: "已连接", title: "已建立的 TCP 连接" },
          { value: "udp", label: "UDP", title: "UDP 端点" },
        ]}
      />
    ) : page === "history" ? (
      <>
        <Segments
          key="history"
          label="时间范围"
          value={historyRange}
          onChange={(v) => {
            setHistoryRange(v);
            setHistoryAt(null);
          }}
          items={historyRanges.map((r) => ({ value: r.value, label: r.label }))}
        />
        {historyAt !== null && (
          <Button onClick={() => setHistoryAt(null)} title="取消选中的时刻，显示最新数据">
            回到最新
          </Button>
        )}
      </>
    ) : null;
  const searchBox = hasSearch && (
    <label
      className={"a-search" + ((queries[page] ?? "").trim() ? " is-filled" : "")}
      data-placed={level < 4 && fit.search ? "" : undefined}
      style={level < 4 && fit.search ? { left: fit.search.left, width: fit.search.width } : undefined}
    >
      <Icon name="search" />
      <input
        ref={search}
        type="search"
        placeholder={searchHint[page]}
        aria-label={searchHint[page]}
        value={queries[page] ?? ""}
        data-tour="search"
        spellCheck={false}
        autoComplete="off"
        onChange={(e) => setQueries((q) => ({ ...q, [page]: e.target.value }))}
      />
      {(queries[page] ?? "") && (
        <button
          type="button"
          className="a-search-clear"
          aria-label="清除搜索"
          title="清除搜索"
          onClick={() => {
            setQueries((q) => ({ ...q, [page]: "" }));
            search.current?.focus();
          }}
        >
          <Icon name="dismiss" size={12} />
        </button>
      )}
    </label>
  );
  const pageIndex = pages.findIndex((p) => p[0] === page);

  return (
    <div
      ref={root}
      className={
        "argus-app" +
        (navLeft ? " is-nav-left" : "") +
        (navLeft && navOpen ? " is-nav-open" : "") +
        (maximized ? " is-max" : "") +
        (dragging ? " is-dragging" : "") +
        (tray ? " is-tray" : "")
      }
      data-ui
      data-window={tray ?? (maximized ? "maximized" : "normal")}
      data-mode={mode}
      tabIndex={-1}
      onFocus={() => (focused.current = true)}
      onBlur={(event) => {
        if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget as Node)) focused.current = false;
      }}
      style={{ ...accent, "--a-rail": navLeft && navOpen ? "176px" : "48px", "--a-text": fontSize / 13 } as CSSProperties}
      onKeyDown={keys}
      onDragOver={(event) => {
        if (event.dataTransfer.types.includes("Files")) {
          event.preventDefault();
          setDragging(true);
        }
      }}
      onDragLeave={(event) => {
        if (event.currentTarget === event.target) setDragging(false);
      }}
      onDrop={drop}
    >
      <div
        className="a-window"
        aria-hidden={tray ? true : undefined}
        inert={tray ? true : undefined}
        style={scale === 100 ? undefined : { zoom: scale / 100 }}
      >
        <div className="a-titlebar" role="toolbar" aria-label="工具栏">
          <div className="a-toolbar" ref={fit.bar}>
            <div className="a-start" ref={fit.start}>
              <div className="a-brand">
                <img src="/assets/argus.svg" width="18" height="18" alt="" draggable={false} />
                <span>Argus</span>
              </div>
              {level < 3 && controls}
            </div>
            <span className="a-grow" />
            <div className="a-trailing" ref={fit.trailing}>
              <Button icon="runTask" title="运行新任务" label="运行新任务" onClick={() => setDialog({ kind: "run" })}>
                {wide ? "运行新任务" : undefined}
              </Button>
              {page === "services" && (
                <>
                  <Button
                    icon="play"
                    title="启动服务"
                    disabled={!service || serviceState !== "stopped"}
                    onClick={() => service && control(service, "start")}
                  />
                  <Button
                    icon="stop"
                    title="停止服务"
                    disabled={!service || serviceState !== "running"}
                    onClick={() => service && control(service, "stop")}
                  />
                  <Button
                    icon="restart"
                    title="重新启动服务"
                    disabled={!service || serviceState !== "running"}
                    onClick={() => service && control(service, "restart")}
                  />
                  <Button
                    icon="external"
                    title="打开 Windows 服务管理器"
                    label="服务管理器"
                    onClick={() => say("演示窗口不会打开 Windows 服务管理器。", "info")}
                  >
                    {wide ? "服务管理器" : undefined}
                  </Button>
                </>
              )}
              {sampled && (
                <>
                  <Select
                    label="刷新频率"
                    value={interval}
                    onChange={setIntervalMs}
                    options={[500, 1000, 2000, 5000].map((ms) => ({ value: ms, label: `每 ${ms / 1000} 秒` }))}
                  />
                  <Button
                    icon={paused ? "play" : "pause"}
                    title={paused ? "继续刷新" : "暂停刷新"}
                    pressed={paused}
                    tour="pause"
                    onClick={() => setPaused(!paused)}
                  />
                </>
              )}
              <Button icon="lock" title="查找文件占用（也可将文件拖放到窗口）" label="查找占用文件的进程" onClick={() => setDialog({ kind: "lock" })} />
              {page === "processes" && (
                <span
                  data-details
                  onContextMenu={(event) => {
                    event.preventDefault();
                    showMenu(
                      { x: event.clientX, y: event.clientY },
                      DETAILS_MODES.map((mode) => ({
                        label: DETAILS_MODE_NAMES[mode],
                        checked: detailsMode === mode,
                        run: () => chooseDetails(mode),
                      })),
                    );
                  }}
                >
                  <Button
                    icon={detailsMode === "auto" ? "panelAuto" : "panel"}
                    title={`详细信息面板：${DETAILS_MODE_NAMES[detailsMode]}\n单击改为“${DETAILS_MODE_NAMES[nextDetails(detailsMode)]}”，右键可直接选择`}
                    label="详细信息面板"
                    pressed={detailsMode !== "closed"}
                    onClick={() => chooseDetails(nextDetails(detailsMode))}
                  />
                </span>
              )}
            </div>
          </div>
          {level < 4 && searchBox}
          <div className="a-caption">
            <button type="button" aria-label="最小化" title="最小化" onClick={() => setTray("tray")}>
              <CaptionIcon name="minimize" />
            </button>
            <button type="button" aria-label={maximized ? "向下还原" : "最大化"} title={maximized ? "向下还原" : "最大化"} onClick={() => setMaximized(!maximized)}>
              <CaptionIcon name={maximized ? "restore" : "maximize"} />
            </button>
            <button type="button" aria-label="关闭" title="关闭" className="is-close" onClick={() => setTray("tray")}>
              <CaptionIcon name="close" />
            </button>
          </div>
        </div>
        {level >= 3 && controls && <div className="a-toolbar-row">{controls}</div>}
        {level >= 4 && searchBox && <div className="a-toolbar-row">{searchBox}</div>}
        <div className="a-frame">
          <Navigation
            page={page}
            side={navLeft}
            open={navOpen}
            onToggle={() => setNavOpen(!navOpen)}
            onPage={go}
          />
          <div className="a-body">
            <main
              key={page}
              ref={content}
              className="a-content"
              aria-label={pages[pageIndex][1]}
            >
              {page === "processes" && (
                <div className="a-processes">
                  <Vitals
                    tick={tick}
                    interval={interval}
                    running={running}
                    motion={motion}
                    onOpen={(d) => {
                      setDevice(d);
                      go("performance");
                    }}
                  />
                  <div className={"a-work" + (query.trim() ? " is-filtered" : "")}>
                    <div className="a-work-clip">
                      <ProcessTable
                        tick={tick}
                        rows={rows}
                        total={alive.length}
                        columns={shownColumns}
                        autoWidth={columnPrefs.autoWidth}
                        text={fontSize / 13}
                        list={alive}
                        priority={priority}
                        sort={sort}
                        descending={descending}
                        selected={selected}
                        filtered={!!query.trim()}
                        efficiency={efficiency}
                        onSort={sortBy}
                        onToggle={toggleRow}
                        onSelect={setSelected}
                        onMenu={openRowMenu}
                        onEnd={end}
                        onDetails={revealDetails}
                        onHold={setHold}
                        onHeaderMenu={openHeaderMenu}
                        onMoveColumn={(key, before) => setColumnPrefs((p) => moveBefore(p, key, before))}
                      />
                      {detailsOpen && (
                        <Details
                          entering={paneEnters}
                          row={selectedRow}
                          list={alive}
                          tick={tick}
                          interval={interval}
                          running={running}
                          motion={motion}
                          history={history}
                          efficiency={efficiency}
                          priority={priority}
                          onDeselect={() => setSelected(null)}
                          onEnd={end}
                          onSelectPid={showPid}
                          onDevice={(d) => {
                            setDevice(d);
                            go("performance");
                          }}
                          onService={(name) => {
                            setService(name);
                            go("services");
                          }}
                          onCopy={copy}
                        />
                      )}
                    </div>
                  </div>
                </div>
              )}
              <>
                {page === "performance" && Performance && (
                  <Performance
                    tick={tick}
                    interval={interval}
                    running={running}
                    motion={motion}
                    device={device}
                    processCount={alive.length}
                    threads={threads}
                    handles={handles}
                    onDevice={setDevice}
                  />
                )}
                {page === "services" && Services && (
                  <Services
                    list={services}
                    states={serviceStates}
                    filter={serviceFilter}
                    query={queries.services ?? ""}
                    selected={service}
                    onSelect={setService}
                    onMenu={serviceMenu}
                  />
                )}
                {page === "startup" && Startup && (
                  <Startup
                    items={startup}
                    query={queries.startup ?? ""}
                    onMenu={(event, items) => showMenu({ x: event.clientX, y: event.clientY }, items)}
                    onToggle={(name) => {
                      const item = startup.find((i) => i.name === name)!;
                      setStartup(startup.map((i) => (i.name === name ? { ...i, enabled: !i.enabled } : i)));
                      say(item.enabled ? `已禁用“${name}”，下次登录时生效。` : `已启用“${name}”，下次登录时生效。`);
                    }}
                  />
                )}
                {page === "connections" && Connections && (
                  <Connections
                    alive={alive}
                    filter={connectionFilter}
                    query={queries.connections ?? ""}
                    onOpen={showPid}
                    onMenu={(event, items) => showMenu({ x: event.clientX, y: event.clientY }, items)}
                  />
                )}
                {page === "history" && History && (
                  <History range={historyRange} cursor={historyAt} onCursor={setHistoryAt} onOpen={showPid} />
                )}
                {page === "settings" && Settings && (
                  <Settings
                    theme={theme}
                    onTheme={setTheme}
                    motion={motion}
                    onMotion={setMotion}
                    navLeft={navLeft}
                    onNav={setNavLeft}
                    interval={interval}
                    onInterval={setIntervalMs}
                    scale={scale}
                    onScale={setScale}
                    fontSize={fontSize}
                    onFontSize={setFontSize}
                    onSay={(text) => say(text, "info")}
                  />
                )}
                {page === "about" && About && (
                  <About onCopy={copy} onQuit={() => setTray("exited")} onSay={(text) => say(text, "info")} />
                )}
              </>
            </main>
            <footer className="a-status">
              <span className="a-status-figures">
                <span>
                  线程 <b>{thousands(threads)}</b>
                </span>
                <span>
                  句柄 <b>{thousands(handles)}</b>
                </span>
                <span>
                  缓存 <b>7.7 GB</b>
                </span>
                <span>
                  分页缓冲池 <b>640.0 MB</b>
                </span>
                <span>
                  已运行 <b>5 天 14 小时</b>
                </span>
              </span>
              {hold && page === "processes" && !paused && (
                <span title="鼠标移出列表后按当前排序重新排列">指针位于列表上，暂停重新排序</span>
              )}
              <span className={"a-pulse" + (paused || (ctrlHeld && sampled) ? " is-paused" : "")}>
                {paused || (ctrlHeld && sampled) ? "已暂停刷新" : `每 ${interval / 1000} 秒刷新`}
              </span>
              <span className="a-privilege" title="Argus 正以管理员身份运行，可查看和结束绝大多数进程">
                <Icon name="shield" size={12} />
                管理员
              </span>
            </footer>
          </div>
        </div>
        {menu && <ContextMenu key={menu.id} menu={menu} onClose={closeMenu} />}
        {dialog && (
          <div
            className="a-scrim"
            ref={focusDialog}
            // The column dialog keeps its draft: only its own buttons and Esc close it.
            onPointerDown={(e) => e.target === e.currentTarget && dialog.kind !== "columns" && setDialog(null)}
            onKeyDown={(event) => {
              // Enter confirms, as the dialog's primary button would.
              const target = event.target as HTMLElement;
              if (event.key === "Enter" && !target.closest("button, input")) {
                event.preventDefault();
                event.currentTarget.querySelector<HTMLButtonElement>("[data-primary]")?.click();
              }
            }}
          >
            {dialog.kind === "lock" && (
              <div className="a-dialog is-lock" role="dialog" tabIndex={-1} aria-modal="true" aria-labelledby="a-dialog-title">
                <div className="a-dialog-body a-scroll">
                  <h3 id="a-dialog-title">谁在占用文件</h3>
                  <p className="a-dialog-text">
                    删除或移动文件时提示“文件正在使用”，可在此查找占用该文件的进程，也可将文件直接拖放到 Argus 窗口。
                  </p>
                  <FileLock
                    result={lock}
                    checking={checking}
                    onFiles={files}
                    onShow={showPid}
                    onEnd={(proc) => end(rowFor(proc), true)}
                    footer={false}
                  />
                </div>
                <div className="a-dialog-foot">
                  <p>句柄查询可发现当前目录占用；受保护进程仍可能无法检查。</p>
                  <button type="button" className="a-btn is-wide" onClick={() => setDialog(null)} data-primary>
                    关闭
                  </button>
                </div>
              </div>
            )}
            {dialog.kind === "run" && (
              <form
                className="a-dialog is-task"
                role="dialog"
                aria-modal="true"
                aria-labelledby="a-dialog-title"
                onSubmit={(e) => {
                  e.preventDefault();
                  if (!command.trim()) return;
                  setDialog(null);
                  say(`演示窗口不会运行“${command.trim()}”。`, "info");
                  setCommand("");
                }}
              >
                <div className="a-task-head">
                  <span className="a-task-glyph">
                    <Icon name="runTask" size={24} />
                  </span>
                  <div>
                    <h3 id="a-dialog-title">运行新任务</h3>
                    <p>打开应用、文件夹或文档，也可输入启动参数。</p>
                  </div>
                  <Button icon="dismiss" title="关闭" onClick={() => setDialog(null)} />
                </div>
                <div className="a-task-body">
                  <label htmlFor="a-task-input">打开</label>
                  <div className="a-task-row">
                    <input
                      id="a-task-input"
                      className="a-input"
                      value={command}
                      onChange={(e) => setCommand(e.target.value)}
                      placeholder="explorer.exe 或程序路径"
                      spellCheck={false}
                      autoComplete="off"
                      autoFocus
                    />
                    <Button icon="folder">浏览…</Button>
                  </div>
                  <div className="a-check">
                    <button
                      type="button"
                      role="checkbox"
                      className="a-check-box"
                      aria-checked={elevated}
                      aria-labelledby="a-task-admin"
                      onClick={() => setElevated(!elevated)}
                    >
                      {elevated && <Icon name="check" size={12} />}
                    </button>
                    <div className="a-check-text" onClick={() => setElevated(!elevated)}>
                      <span id="a-task-admin">
                        以管理员身份运行
                        <Icon name="shield" size={14} />
                      </span>
                      <small>{elevated ? "需要 Windows 授权；取消授权后不会运行。" : "未勾选时，以普通权限运行。"}</small>
                    </div>
                  </div>
                  <div style={{ minHeight: 54 }} />
                </div>
                <div className="a-dialog-foot">
                  <button type="button" className="a-btn is-wide" onClick={() => setDialog(null)}>
                    取消
                  </button>
                  <button type="submit" className="a-btn is-accent is-wide" data-primary>
                    运行
                  </button>
                </div>
              </form>
            )}
            {dialog.kind === "end" && (
              <div className="a-dialog" role="alertdialog" tabIndex={-1} aria-modal="true" aria-labelledby="a-dialog-title">
                <div className="a-dialog-body">
                  <h3 id="a-dialog-title">
                    {dialog.tree ? `结束 ${dialog.row.label} 的进程树？` : `结束 ${dialog.row.label}？`}
                  </h3>
                  <p className="a-dialog-text">
                    {dialog.tree
                      ? descendants(dialog.row.pids[0]).length
                        ? `将结束 PID ${dialog.row.pids[0]} 及其 ${descendants(dialog.row.pids[0]).length} 个子进程，未保存的数据会丢失。`
                        : `PID ${dialog.row.pids[0]} 没有子进程，仅结束该进程。`
                      : dialog.row.count > 1
                        ? `将结束 ${dialog.row.label} 的全部 ${dialog.row.count} 个进程，未保存的数据会丢失。`
                        : `PID ${dialog.row.pids[0]}。未保存的数据会丢失。`}
                  </p>
                </div>
                <div className="a-dialog-foot">
                  <button type="button" className="a-btn is-danger is-wide" onClick={() => finish(dialog.row, dialog.tree)} data-primary>
                    {dialog.tree ? "结束进程树" : dialog.row.count > 1 ? "全部结束" : "结束进程"}
                  </button>
                  <button type="button" className="a-btn is-wide" onClick={() => setDialog(null)}>
                    取消
                  </button>
                </div>
              </div>
            )}
            {dialog.kind === "realtime" && (
              <div className="a-dialog" role="alertdialog" tabIndex={-1} aria-modal="true" aria-labelledby="a-dialog-title">
                <div className="a-dialog-body">
                  <h3 id="a-dialog-title">将 {dialog.row.label} 设为实时优先级？</h3>
                  <p className="a-dialog-text">实时优先级的进程优先于系统任务运行，占满处理器时鼠标和键盘可能失去响应。</p>
                </div>
                <div className="a-dialog-foot">
                  <button
                    type="button"
                    className="a-btn is-danger is-wide"
                    data-primary
                    onClick={() => {
                      setPriorityOf(dialog.row, "实时");
                      setDialog(null);
                      say(`${dialog.row.label} 的优先级已设为“实时”。`);
                    }}
                  >
                    设为实时
                  </button>
                  <button type="button" className="a-btn is-wide" onClick={() => setDialog(null)}>
                    取消
                  </button>
                </div>
              </div>
            )}
            {dialog.kind === "rule" && (
              <div className="a-dialog" role="alertdialog" tabIndex={-1} aria-modal="true" aria-labelledby="a-dialog-title">
                <div className="a-dialog-body">
                  <h3 id="a-dialog-title">为所有同名进程添加规则？</h3>
                  <p className="a-dialog-text">
                    规则按可执行文件名 {dialog.row.exe} 匹配，会作用于所有同名进程，不限于当前宿主或安装路径。
                  </p>
                </div>
                <div className="a-dialog-foot">
                  <button
                    type="button"
                    className="a-btn is-accent is-wide"
                    data-primary
                    onClick={() => {
                      setDialog(null);
                      go("settings");
                    }}
                  >
                    继续设置规则
                  </button>
                  <button type="button" className="a-btn is-wide" onClick={() => setDialog(null)}>
                    取消
                  </button>
                </div>
              </div>
            )}
            {dialog.kind === "columns" && (
              <ColumnsDialog
                prefs={columnPrefs}
                onApply={(next) => {
                  setColumnPrefs(next);
                  setDialog(null);
                }}
                onClose={() => setDialog(null)}
              />
            )}
          </div>
        )}
        {dragging && (
          <div className="a-dropping">
            <Icon name="lock" size={28} />
            释放鼠标以查找占用这些文件的进程
          </div>
        )}
        {toast && (
          <div className="a-toast" role="status" key={toast.id}>
            <Icon
              name={toast.tone === "error" ? "warning" : toast.tone === "info" ? "info" : "check"}
              className={toast.tone === "error" ? "is-error" : undefined}
            />
            <p>{toast.text}</p>
            <button type="button" className="a-icon-btn" aria-label="关闭" onClick={() => setToast(null)}>
              <Icon name="dismiss" size={12} />
            </button>
          </div>
        )}
      </div>
      {tray && (
        <div className="a-tray">
          <p>
            {tray === "tray" ? "Argus 已隐藏到托盘。界面暂停绘制，采样、历史记录与规则继续运行。" : "Argus 已退出。"}
          </p>
          <button
            type="button"
            className="a-tray-icon"
            onClick={() => setTray(null)}
            title={`Argus · CPU ${Math.round(s.cpu)}% · 内存 ${Math.round(s.mem)}%`}
          >
            <img src="/assets/argus.svg" width="18" height="18" alt="" />
            <span>{tray === "tray" ? "单击托盘图标或按 Ctrl+Alt+Esc 显示" : "重新启动 Argus"}</span>
          </button>
        </div>
      )}
    </div>
  );
}

// Tabs under the toolbar, or a rail of icons at the side; the accent mark glides to the chosen page.
function Navigation({
  page,
  side,
  open,
  onToggle,
  onPage,
}: {
  page: Page;
  side: boolean;
  open: boolean;
  onToggle: () => void;
  onPage: (page: Page) => void;
}) {
  const tabs = useRef<HTMLElement>(null);
  const [mark, setMark] = useState<{ at: number; moving: boolean } | null>(null);
  const placed = useRef(false);
  const layout = useRef("");
  useIsoLayoutEffect(() => {
    const el = tabs.current;
    if (!el) return;
    const place = (moving: boolean) => {
      const chosen = el.querySelector<HTMLElement>("button[aria-current]");
      if (!chosen) return;
      const at = side ? chosen.offsetTop + chosen.offsetHeight / 2 - 8 : chosen.offsetLeft + chosen.offsetWidth / 2 - 8;
      setMark((m) => (m && m.at === at ? m : { at, moving }));
    };
    const key = `${side}|${open}`;
    place(placed.current && layout.current === key);
    placed.current = true;
    layout.current = key;
    const observer = new ResizeObserver(() => place(false));
    observer.observe(el);
    return () => observer.disconnect();
  }, [page, side, open]);
  return (
    <div className="a-nav-pane">
      {side && (
        <button
          type="button"
          className="a-nav-toggle"
          aria-label={open ? "收起导航" : "展开导航"}
          title={open ? "收起导航" : "展开导航"}
          aria-expanded={open}
          onClick={onToggle}
        >
          <Icon name="menu" />
        </button>
      )}
      <nav className="a-nav a-nav-tabs" ref={tabs} aria-label="页面" role="tablist">
        {pages.map(([id, label, icon], i) => (
          <span key={id} style={{ display: "contents" }}>
            {id === "settings" && <span className="a-nav-spacer" />}
            <button
              type="button"
              role="tab"
              aria-selected={page === id}
              aria-current={page === id ? "page" : undefined}
              title={`${label}（Ctrl+${i + 1}）`}
              data-tour={"nav-" + id}
              data-i18n="nav"
              onClick={() => onPage(id)}
            >
              <Icon name={icon} className="a-nav-icon" />
              <span className="a-nav-label" data-text={label} data-i18n="nav">
                <span data-i18n="nav">{label}</span>
              </span>
            </button>
          </span>
        ))}
        {mark && (
          <span
            className={"a-nav-mark" + (mark.moving ? " is-moving" : "")}
            style={{ transform: side ? `translateY(${mark.at}px)` : `translateX(${mark.at}px)` }}
            aria-hidden="true"
          />
        )}
      </nav>
    </div>
  );
}

// Flips a check that keeps its menu open where it is drawn (menus.rs
// `toggle_in_place`); a submenu's owner showing a count of its checked entries
// counts again.
function flipInPlace(items: MenuEntry[], owner: number | null, index: number): MenuEntry[] {
  const flip = (list: MenuEntry[]) =>
    list.map((entry, i) => (i === index && entry !== "-" ? { ...entry, checked: !entry.checked } : entry));
  if (owner === null) return flip(items);
  return items.map((entry, i) => {
    if (i !== owner || entry === "-") return entry;
    const children = flip(entry.children ?? []);
    const shown = children.filter((c) => c !== "-");
    const detail = entry.detail === undefined ? undefined : `${shown.filter((c) => c.checked).length}/${shown.length}`;
    return { ...entry, children, detail };
  });
}

// A context menu with one level of submenu: 30 px items, 9 px separators; the
// submenu opens beside its item after a short hover, or with → and Enter. Its
// highlight lives here, so moving over it redraws only the menu.
function ContextMenu({ menu, onClose }: { menu: MenuState; onClose: () => void }) {
  const box = useRef<HTMLDivElement>(null);
  const subBox = useRef<HTMLDivElement>(null);
  // Kept here: a check that leaves the menu open flips where it is drawn.
  const [items, setItems] = useState(menu.items);
  const usable = (list: MenuEntry[]) => list.map((e, i) => (e !== "-" && !e.disabled ? i : -1)).filter((i) => i >= 0);
  const [focus, setFocus] = useState<number | null>(menu.keyboard ? (usable(items)[0] ?? null) : null);
  const [sub, setSub] = useState<number | null>(null);
  const [subFocus, setSubFocus] = useState<number | null>(null);
  const [keyboard, setKeyboard] = useState(menu.keyboard);
  const [place, setPlace] = useState<{ x: number; y: number } | null>(null);
  const [subPlace, setSubPlace] = useState<{ x: number; y: number } | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const children = sub !== null ? ((items[sub] as Exclude<MenuEntry, "-">).children ?? []) : null;
  useIsoLayoutEffect(() => {
    const el = box.current;
    const frame = el?.closest(".a-window") as HTMLElement | null;
    if (!el || !frame) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    const x = menu.x + w > frame.clientWidth - 8 ? Math.max(8, menu.x - w) : menu.x;
    const y = menu.y + h > frame.clientHeight - 8 ? Math.max(8, menu.y - h) : menu.y;
    setPlace({ x, y });
    el.focus({ preventScroll: true });
  }, [menu]);
  useIsoLayoutEffect(() => {
    if (sub === null || !place) {
      setSubPlace(null);
      return;
    }
    const el = box.current;
    const sb = subBox.current;
    const frame = el?.closest(".a-window") as HTMLElement | null;
    const owner = el?.querySelector<HTMLElement>(`[data-entry="${sub}"]`);
    if (!el || !sb || !frame || !owner) return;
    // Beside its owner toward the free side, its first item level with the owner.
    const right = place.x + el.offsetWidth - 5;
    const x = right + sb.offsetWidth > frame.clientWidth - 8 ? Math.max(8, place.x + 5 - sb.offsetWidth + 4) : right - 4;
    const y = Math.max(8, Math.min(frame.clientHeight - 8 - sb.offsetHeight, place.y + owner.offsetTop - 4));
    setSubPlace({ x, y });
  }, [sub, place]);
  useEffect(() => {
    const close = (event: Event) => {
      if (!(event.target instanceof Element) || !event.target.closest(".a-menu")) onClose();
    };
    const blur = () => onClose();
    document.addEventListener("pointerdown", close, true);
    window.addEventListener("blur", blur);
    return () => {
      document.removeEventListener("pointerdown", close, true);
      window.removeEventListener("blur", blur);
      clearTimeout(timer.current);
    };
  }, [onClose]);
  const activate = (entry: MenuEntry, index: number, inSub: boolean) => {
    if (entry === "-" || entry.disabled) return;
    if (!inSub && entry.children) {
      clearTimeout(timer.current);
      setSub(index);
      setSubFocus(keyboard ? 0 : null);
      return;
    }
    if (entry.stay) {
      setItems((list) => flipInPlace(list, inSub ? sub : null, index));
      entry.run?.();
      return;
    }
    onClose();
    entry.run?.();
  };
  // The submenu opens or closes after the pointer rests (Windows waits about as long).
  const later = (next: number | null) => {
    clearTimeout(timer.current);
    if (sub === next) return;
    timer.current = setTimeout(() => {
      setSub(next);
      setSubFocus(null);
    }, 250);
  };
  const keys = (event: KeyboardEvent<HTMLDivElement>) => {
    const inSub = sub !== null && subFocus !== null;
    const list = inSub ? children! : items;
    const choices = usable(list);
    const current = inSub ? subFocus : focus;
    const move = (to: number) => {
      setKeyboard(true);
      if (inSub) setSubFocus(to);
      else setFocus(to);
    };
    const at = current === null ? -1 : choices.indexOf(current);
    switch (event.key) {
      case "ArrowDown":
        move(choices[(at + 1) % choices.length]);
        break;
      case "ArrowUp":
        move(choices[(at - 1 + choices.length) % choices.length]);
        break;
      case "Home":
        move(choices[0]);
        break;
      case "End":
        move(choices[choices.length - 1]);
        break;
      case "ArrowRight":
        if (!inSub && focus !== null && (items[focus] as Exclude<MenuEntry, "-">).children) {
          setKeyboard(true);
          setSub(focus);
          setSubFocus(usable((items[focus] as Exclude<MenuEntry, "-">).children!)[0] ?? 0);
        }
        break;
      case "ArrowLeft":
        if (inSub) {
          setSub(null);
          setSubFocus(null);
        }
        break;
      case "Enter":
      case " ":
        if (current !== null) activate(list[current], current, inSub);
        break;
      case "Escape":
        if (inSub) {
          setSub(null);
          setSubFocus(null);
        } else onClose();
        break;
      case "Tab":
        onClose();
        return;
      default:
        return;
    }
    event.preventDefault();
    event.stopPropagation();
  };
  const render = (list: MenuEntry[], inSub: boolean) =>
    list.map((entry, index) => {
      if (entry === "-") return <hr key={"sep" + index} />;
      const owner = !inSub && !!entry.children;
      const lit = inSub ? subFocus === index : focus === index || (owner && sub === index);
      const glyph: UiIcon | undefined = entry.checked ? "check" : entry.icon;
      return (
        <button
          key={entry.label}
          type="button"
          data-entry={index}
          role={entry.checked !== undefined ? "menuitemcheckbox" : "menuitem"}
          aria-checked={entry.checked}
          aria-haspopup={owner ? "menu" : undefined}
          aria-expanded={owner ? sub === index : undefined}
          aria-keyshortcuts={entry.hint}
          disabled={entry.disabled}
          className={(entry.danger ? "is-danger" : "") + (lit ? " is-lit" : "")}
          onPointerEnter={() => {
            setKeyboard(false);
            if (inSub) {
              clearTimeout(timer.current);
              setSubFocus(index);
            } else {
              setFocus(index);
              later(owner ? index : null);
            }
          }}
          onPointerLeave={() => (inSub ? setSubFocus(null) : setFocus(null))}
          onClick={() => activate(entry, index, inSub)}
          tabIndex={-1}
        >
          <span className="a-menu-icon">{glyph && <Icon name={glyph} />}</span>
          {entry.label}
          {entry.hint && <kbd>{entry.hint}</kbd>}
          {entry.detail && <span className="a-menu-detail">{entry.detail}</span>}
          {owner && (
            <span className="a-menu-more">
              <Icon name="chevronRight" size={12} />
            </span>
          )}
        </button>
      );
    });
  return (
    <>
      <div
        ref={box}
        className="a-menu"
        role="menu"
        tabIndex={-1}
        onKeyDown={keys}
        onContextMenu={(event) => event.preventDefault()}
        style={{ left: place?.x ?? menu.x, top: place?.y ?? menu.y, visibility: place ? undefined : "hidden" }}
      >
        {render(items, false)}
      </div>
      {children && (
        <div
          ref={subBox}
          className="a-menu"
          role="menu"
          onPointerEnter={() => clearTimeout(timer.current)}
          onContextMenu={(event) => event.preventDefault()}
          style={{ left: subPlace?.x ?? 0, top: subPlace?.y ?? 0, visibility: subPlace ? undefined : "hidden" }}
        >
          {render(children, true)}
        </div>
      )}
    </>
  );
}
