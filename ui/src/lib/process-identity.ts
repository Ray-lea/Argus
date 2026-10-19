// Group membership shared by rows, history, details and process actions.
import type { ProcessRow } from "./types.ts";

type Identity = Pick<ProcessRow, "pid" | "createTime" | "name" | "path" | "webView">;

// Their executable identifies a host, not the app or task running inside it.
const SHARED_HOSTS = new Set([
  "msedgewebview2.exe",
  "svchost.exe",
  "dllhost.exe",
  "rundll32.exe",
  "conhost.exe",
  "runtimebroker.exe",
  "applicationframehost.exe",
  "taskhostw.exe",
  "taskhost.exe",
  "wudfhost.exe",
  "node.exe",
  "python.exe",
  "pythonw.exe",
  "java.exe",
  "javaw.exe",
  "dotnet.exe",
  "wscript.exe",
  "cscript.exe",
  "cmd.exe",
  "powershell.exe",
  "pwsh.exe",
]);

export const procKey = (p: Pick<ProcessRow, "pid" | "createTime">) => `${p.pid}:${p.createTime}`;
export const processRowId = (key: string) => `row-${encodeURIComponent(key)}`;

function imageKey(path: string): string {
  return path
    .replaceAll("/", "\\")
    .replace(/^\\\\\?\\UNC\\/i, "\\\\")
    .replace(/^\\\\\?\\/, "")
    .toLowerCase();
}

export const sharedHost = (p: Pick<ProcessRow, "name">) => SHARED_HOSTS.has(p.name.toLowerCase());
export const canGroup = (p: Identity) => !!p.webView || (!!p.path && !sharedHost(p));

export function applicationKey(p: Identity): string {
  if (p.webView) return `g:webview:${p.webView.browserPid}:${p.webView.browserCreateTime}`;
  if (!p.path || sharedHost(p)) return `g:instance:${procKey(p)}`;
  return `g:image:${imageKey(p.path)}`;
}

export const applicationMembers = (processes: readonly ProcessRow[], key: string) =>
  processes.filter((p) => applicationKey(p) === key);

/** Rules intentionally match the executable name, independently of display groups. */
export const broadRuleScope = (p: ProcessRow, processes: readonly ProcessRow[]) =>
  sharedHost(p) ||
  processes.some(
    (other) =>
      other.name.toLowerCase() === p.name.toLowerCase() && applicationKey(other) !== applicationKey(p),
  );
