// User-facing process operations: confirm, run, report.

import { api } from "./ipc";
import type { DisplayRow } from "./rows";
import { markEnding, snapshot } from "./store";
import type { ActionError, ProcessRow } from "./types";
import { confirm, toast } from "./ui";

function errorText(e: unknown, name: string): string {
  const err = e as ActionError;
  switch (err?.kind) {
    case "accessDenied":
      return `没有权限结束 ${name}。以管理员身份运行 Argus 后可以结束它。`;
    case "notFound":
      return `${name} 已经退出。`;
    case "critical":
      return `${name} 是系统关键进程，结束它会导致系统崩溃，已阻止。`;
    case "refused":
      return `不能结束 ${name}。`;
    case "os":
      return `无法结束 ${name}：${err.message}`;
    default:
      return `无法结束 ${name}：${String(e)}`;
  }
}

/** Dims the row right away; it clears when the process is gone (or the attempt failed). */
async function whileEnding<T>(key: string, run: () => Promise<T>, ok: (result: T) => boolean): Promise<T> {
  markEnding(key, true);
  try {
    const result = await run();
    if (ok(result)) setTimeout(() => markEnding(key, false), 3000);
    else markEnding(key, false);
    return result;
  } catch (e) {
    markEnding(key, false);
    throw e;
  }
}

function groupMembers(row: DisplayRow): ProcessRow[] {
  const name = row.proc.name.toLowerCase();
  return snapshot()?.processes.filter((p) => p.name.toLowerCase() === name) ?? [];
}

function descendantCount(root: ProcessRow): number {
  const procs = snapshot()?.processes ?? [];
  const byParent = new Map<number, ProcessRow[]>();
  for (const p of procs) {
    const list = byParent.get(p.ppid);
    if (list) list.push(p);
    else byParent.set(p.ppid, [p]);
  }
  let n = 0;
  const stack = [root];
  const seen = new Set<number>([root.pid]);
  while (stack.length) {
    const p = stack.pop() as ProcessRow;
    for (const c of byParent.get(p.pid) ?? []) {
      if (c.createTime >= p.createTime && !seen.has(c.pid)) {
        seen.add(c.pid);
        n++;
        stack.push(c);
      }
    }
  }
  return n;
}

/** Ends a process, or every process of an app group. */
export async function endRow(row: DisplayRow, skipConfirm = false) {
  if (row.kind === "group") {
    const members = groupMembers(row);
    if (
      !skipConfirm &&
      !(await confirm({
        title: `结束 ${row.label}？`,
        body: `将结束 ${row.proc.name} 的全部 ${members.length} 个进程，未保存的数据会丢失。`,
        confirmLabel: "全部结束",
        danger: true,
      }))
    ) {
      return;
    }
    const results = await whileEnding(
      row.key,
      () => Promise.allSettled(members.map((p) => api.kill(p.pid, p.createTime))),
      (r) => r.some((x) => x.status === "fulfilled"),
    );
    const failed = results.filter((r) => r.status === "rejected") as PromiseRejectedResult[];
    if (failed.length === 0) toast("success", `已结束 ${row.label} 的 ${members.length} 个进程。`);
    else toast("error", `${errorText(failed[0].reason, row.label)}（${failed.length} 个未结束）`);
    return;
  }

  const p = row.proc;
  if (
    !skipConfirm &&
    !(await confirm({
      title: `结束 ${p.name}？`,
      body: `PID ${p.pid}。未保存的数据会丢失。`,
      confirmLabel: "结束进程",
      danger: true,
    }))
  ) {
    return;
  }
  try {
    await whileEnding(
      row.key,
      () => api.kill(p.pid, p.createTime),
      () => true,
    );
    toast("success", `已结束 ${p.name}。`, 2500);
  } catch (e) {
    toast("error", errorText(e, p.name));
  }
}

export async function endTree(row: DisplayRow) {
  const p = row.proc;
  const n = descendantCount(p);
  const ok = await confirm({
    title: `结束 ${p.name} 的进程树？`,
    body:
      n > 0
        ? `将结束 PID ${p.pid} 及其 ${n} 个子进程，未保存的数据会丢失。`
        : `PID ${p.pid} 没有子进程，只会结束它本身。`,
    confirmLabel: "结束进程树",
    danger: true,
  });
  if (!ok) return;
  try {
    const report = await whileEnding(
      row.key,
      () => api.killTree(p.pid, p.createTime),
      (r) => r.terminated > 0,
    );
    if (report.failed > 0) {
      toast("error", `已结束 ${report.terminated} 个进程，${report.failed} 个没有权限或已退出。`);
    } else {
      toast("success", `已结束 ${report.terminated} 个进程。`, 2500);
    }
  } catch (e) {
    toast("error", errorText(e, p.name));
  }
}

export async function copyText(text: string, what: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast("success", `已复制${what}。`, 1800);
  } catch {
    toast("error", `无法访问剪贴板，${what}没有复制。`);
  }
}

export async function revealInExplorer(path: string) {
  try {
    await api.reveal(path);
  } catch (e) {
    toast("error", `无法打开文件位置：${String(e)}`);
  }
}

/** Reports a failed operation in plain terms, e.g. "无法停止“Print Spooler”：需要管理员权限。" */
export function toastActionError(e: unknown, what: string) {
  const err = e as ActionError;
  switch (err?.kind) {
    case "accessDenied":
      toast("error", `${what}：需要管理员权限。以管理员身份运行 Argus 后再试。`);
      break;
    case "notFound":
      toast("error", `${what}：目标已经不存在。`);
      break;
    case "os":
      toast("error", `${what}：${err.message}`);
      break;
    default:
      toast("error", `${what}：${String(e)}`);
  }
}

/** Ends one process by PID (connections and file locks have no table row to act on). */
export async function endProcessByPid(pid: number, name: string) {
  const proc = snapshot()?.processes.find((p) => p.pid === pid);
  const label = proc?.name ?? name;
  const ok = await confirm({
    title: `结束 ${label}？`,
    body: `PID ${pid}。未保存的数据会丢失。`,
    confirmLabel: "结束进程",
    danger: true,
  });
  if (!ok) return false;
  try {
    await api.kill(pid, proc?.createTime ?? 0);
    toast("success", `已结束 ${label}。`, 2500);
    return true;
  } catch (e) {
    toast("error", errorText(e, label));
    return false;
  }
}
