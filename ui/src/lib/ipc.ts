import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  Connection,
  LockReport,
  ProcessDetails,
  ServiceInfo,
  SettingsView,
  Snapshot,
  StartupItem,
  TreeKillReport,
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
  reveal: (path: string): Promise<void> =>
    mock ? Promise.resolve() : invoke("reveal_in_explorer", { path }),
  settings: (): Promise<SettingsView> => (mock ? mock.settings.get() : invoke("get_settings")),
  setCloseToTray: (value: boolean): Promise<SettingsView> =>
    mock ? mock.settings.update({ closeToTray: value }) : invoke("set_close_to_tray", { value }),
  setHotkey: (hotkey: string): Promise<SettingsView> =>
    mock ? mock.settings.update({ hotkey }) : invoke("set_hotkey", { hotkey }),
  setAutostart: (enabled: boolean): Promise<SettingsView> =>
    mock ? mock.settings.update({ autostart: enabled }) : invoke("set_autostart", { enabled }),
  quit: (): Promise<void> => (mock ? Promise.resolve() : invoke("quit")),

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
