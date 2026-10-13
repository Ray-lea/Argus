import { Channel, invoke } from "@tauri-apps/api/core";
import type { Language } from "./i18n";
import type {
  AppInfo,
  Connection,
  HistoryMoment,
  HistoryPoint,
  HistoryUsage,
  LockReport,
  PriorityClass,
  PrivilegedServiceAction,
  PrivilegedServiceStatus,
  ProcessDetails,
  ProcessRule,
  RuleStats,
  ServiceInfo,
  SettingsView,
  Snapshot,
  StartupItem,
  TaskManagerStatus,
  Theme,
  TreeKillReport,
  UpdateChannel,
  UpdateRoute,
  UpdateStatus,
} from "./types";

/** False when the UI runs in a plain browser (`pnpm dev`), where mock data stands in. */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const mock = inTauri ? null : await import("./mock");

export async function subscribe(onSnapshot: (s: Snapshot) => void): Promise<void> {
  if (mock) return mock.subscribe(onSnapshot);
  const channel = new Channel<Snapshot>();
  channel.onmessage = onSnapshot;
  await invoke("subscribe", { onSnapshot: channel });
}

export const api = {
  appInfo: (): Promise<AppInfo> => (mock ? mock.appInfo() : invoke("app_info")),
  setInterval: (ms: number): Promise<void> => (mock ? mock.setInterval(ms) : invoke("set_interval", { ms })),
  setPaused: (paused: boolean): Promise<void> =>
    mock ? mock.setPaused(paused) : invoke("set_paused", { paused }),
  refresh: (): Promise<void> => (mock ? mock.refresh() : invoke("refresh")),
  details: (pid: number, path: string | null): Promise<ProcessDetails> =>
    mock ? mock.details(pid, path) : invoke("process_details", { pid, path }),
  kill: (pid: number, createTime: number, force = false): Promise<void> =>
    mock ? mock.kill(pid) : invoke("kill_process", { pid, createTime, force }),
  killTree: (pid: number, createTime: number): Promise<TreeKillReport> =>
    mock ? mock.killTree(pid) : invoke("kill_process_tree", { pid, createTime }),
  setPriority: (pid: number, createTime: number, priority: PriorityClass): Promise<PriorityClass> =>
    mock ? mock.setPriority(pid, priority) : invoke("set_priority", { pid, createTime, priority }),
  setEfficiency: (pid: number, createTime: number, on: boolean): Promise<void> =>
    mock ? mock.setEfficiency(pid, on) : invoke("set_efficiency_mode", { pid, createTime, on }),
  reveal: (path: string): Promise<void> =>
    mock ? Promise.resolve() : invoke("reveal_in_explorer", { path }),
  settings: (): Promise<SettingsView> => (mock ? mock.settings.get() : invoke("get_settings")),
  setLanguage: (language: Language): Promise<SettingsView> =>
    mock ? mock.settings.update({ language }) : invoke("set_language", { language }),
  setTheme: (theme: Theme): Promise<SettingsView> =>
    mock ? mock.settings.update({ theme }) : invoke("set_theme", { theme }),
  /** Records an error the page did not handle (the host caps and scrubs it). */
  logError: (message: string): void => {
    if (!mock) void invoke("log_ui_error", { message }).catch(() => {});
  },
  /** Where the caption's maximize button is (CSS px, `right` from the window's right edge). */
  setCaptionButton: (rect: { right: number; top: number; width: number; height: number }): Promise<void> =>
    mock ? Promise.resolve() : invoke("set_caption_button", { rect }),
  completeOnboarding: (version: number): Promise<SettingsView> =>
    mock ? mock.settings.update({ onboarding: version }) : invoke("complete_onboarding", { version }),
  privilegedServiceStatus: (): Promise<PrivilegedServiceStatus> =>
    mock ? mock.privilegedService.status() : invoke("privileged_service_status"),
  managePrivilegedService: (action: PrivilegedServiceAction): Promise<PrivilegedServiceStatus> =>
    mock ? mock.privilegedService.manage(action) : invoke("manage_privileged_service", { action }),
  taskManagerStatus: (): Promise<TaskManagerStatus> =>
    mock ? mock.taskManager.status() : invoke("task_manager_status"),
  manageTaskManager: (action: "enable" | "restore"): Promise<TaskManagerStatus> =>
    mock ? mock.taskManager.manage(action) : invoke("manage_task_manager", { action }),
  taskManagerReady: (): Promise<void> => (mock ? Promise.resolve() : invoke("task_manager_ready")),
  setCloseToTray: (value: boolean): Promise<SettingsView> =>
    mock ? mock.settings.update({ closeToTray: value }) : invoke("set_close_to_tray", { value }),
  setHotkey: (hotkey: string): Promise<SettingsView> =>
    mock ? mock.settings.update({ hotkey }) : invoke("set_hotkey", { hotkey }),
  setAutostart: (enabled: boolean): Promise<SettingsView> =>
    mock ? mock.settings.update({ autostart: enabled }) : invoke("set_autostart", { enabled }),
  setRules: (rules: ProcessRule[]): Promise<SettingsView> =>
    mock ? mock.settings.update({ rules }) : invoke("set_rules", { rules }),
  ruleStats: (): Promise<Record<string, RuleStats>> => (mock ? Promise.resolve({}) : invoke("rule_stats")),
  historyRange: (): Promise<[number, number] | null> =>
    mock ? mock.history.range() : invoke("history_range"),
  historySeries: (from: number, to: number, buckets: number): Promise<HistoryPoint[]> =>
    mock ? mock.history.series(from, to, buckets) : invoke("history_series", { from, to, buckets }),
  historyMoment: (ts: number): Promise<HistoryMoment | null> =>
    mock ? mock.history.moment(ts) : invoke("history_moment", { ts }),
  historyUsage: (): Promise<HistoryUsage> => (mock ? mock.history.usage() : invoke("history_usage")),
  setHistoryEnabled: (enabled: boolean): Promise<SettingsView> =>
    mock ? mock.settings.update({ history: enabled }) : invoke("set_history_enabled", { enabled }),
  clearHistory: (): Promise<void> => (mock ? mock.history.clear() : invoke("clear_history")),
  quit: (): Promise<void> => (mock ? Promise.resolve() : invoke("quit")),

  updateStatus: (): Promise<UpdateStatus> => (mock ? mock.updates.status() : invoke("update_status")),
  checkUpdate: (): Promise<UpdateStatus> => (mock ? mock.updates.check() : invoke("check_update")),
  installUpdate: (): Promise<void> => (mock ? mock.updates.install() : invoke("install_update")),
  setUpdateSettings: (s: {
    check: boolean;
    channel: UpdateChannel;
    route: UpdateRoute;
    proxy: string;
  }): Promise<SettingsView> =>
    mock
      ? mock.settings.update({
          updateCheck: s.check,
          updateChannel: s.channel,
          updateRoute: s.route,
          updateProxy: s.proxy,
        })
      : invoke("set_update_settings", s),
  openReleasePage: (version: string): Promise<void> =>
    mock ? Promise.resolve() : invoke("open_release_page", { version }),
  openLogDir: (): Promise<void> => (mock ? Promise.resolve() : invoke("open_log_dir")),

  services: (): Promise<ServiceInfo[]> => (mock ? mock.m3.services() : invoke("list_services")),
  controlService: (name: string, action: "start" | "stop" | "restart"): Promise<void> =>
    mock ? mock.m3.controlService(name, action) : invoke("control_service", { name, action }),
  setServiceStartType: (
    name: string,
    startType: "auto" | "autoDelayed" | "manual" | "disabled",
  ): Promise<void> =>
    mock ? mock.m3.setStartType(name, startType) : invoke("set_service_start_type", { name, startType }),
  openServicesConsole: (): Promise<void> => (mock ? Promise.resolve() : invoke("open_services_console")),

  startup: (): Promise<StartupItem[]> => (mock ? mock.m3.startup() : invoke("list_startup")),
  setStartupEnabled: (id: string, enabled: boolean): Promise<void> =>
    mock ? mock.m3.setStartupEnabled(id, enabled) : invoke("set_startup_enabled", { id, enabled }),

  connections: (): Promise<Connection[]> => (mock ? mock.m3.connections() : invoke("list_connections")),
  closeConnection: (c: Connection): Promise<void> =>
    mock
      ? Promise.reject({ kind: "accessDenied" })
      : invoke("close_connection", {
          localAddress: c.localAddress,
          localPort: c.localPort,
          remoteAddress: c.remoteAddress ?? "",
          remotePort: c.remotePort ?? 0,
        }),

  findLockers: (paths: string[]): Promise<LockReport> =>
    mock ? mock.m3.findLockers(paths) : invoke("find_lockers", { paths }),
  pickPath: (folder: boolean): Promise<string | null> =>
    mock
      ? Promise.resolve(folder ? "D:\\Projects\\demo" : "C:\\Users\\me\\report.xlsx")
      : invoke("pick_path", { folder }),
};

export const iconUrl = (id: number) => `http://argus.localhost/icon/${id}`;
