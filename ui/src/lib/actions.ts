// User-facing process operations: confirm, run, report.

import { api } from "./ipc";
import type { DisplayRow } from "./rows";
import { markEnding, setFocusRule, setPage, snapshot } from "./store";
import type { ActionError, PriorityClass, ProcessRow, ProcessRule } from "./types";
import { confirm, toast } from "./ui";

export const PRIORITY_LABEL: Record<PriorityClass, string> = {
  realtime: "实时",
  high: "高",
  aboveNormal: "高于正常",
  normal: "正常",
  belowNormal: "低于正常",
  idle: "低",
};

/** The class behind a base priority as the process list reports it (4, 6, 8, 10, 13, 24). */
export function priorityOf(basePriority: number): PriorityClass | null {
  switch (basePriority) {
    case 4:
      return "idle";
    case 6:
      return "belowNormal";
    case 8:
      return "normal";
    case 10:
      return "aboveNormal";
    case 13:
      return "high";
    case 24:
      return "realtime";
    default:
      return null;
  }
}

function errorText(e: unknown, name: string): string {
  const err = e as ActionError;
  switch (err?.kind) {
    case "accessDenied":
      return `没有权限结束 ${name}。可在设置中启用特权服务，或以管理员身份运行 Argus；受保护的进程仍可能无法结束。`;
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

/** The processes an action on `row` applies to: one, or every member of an app group. */
const targetsOf = (row: DisplayRow) => (row.kind === "group" ? groupMembers(row) : [row.proc]);

/** Sets the priority class of a process, or of every process in an app group. */
export async function setRowPriority(row: DisplayRow, priority: PriorityClass) {
  if (
    priority === "realtime" &&
    !(await confirm({
      title: `将 ${row.label} 设为实时优先级？`,
      body: "实时优先级的进程会抢在系统任务之前运行，占满处理器时鼠标和键盘都可能失去响应。",
      confirmLabel: "设为实时",
      danger: true,
    }))
  ) {
    return;
  }
  const targets = targetsOf(row);
  const results = await Promise.allSettled(
    targets.map((p) => api.setPriority(p.pid, p.createTime, priority)),
  );
  const failed = results.filter((r) => r.status === "rejected") as PromiseRejectedResult[];
  if (failed.length === targets.length) {
    toastActionError(failed[0].reason, `无法更改 ${row.label} 的优先级`);
    return;
  }
  const downgraded = results.some((r) => r.status === "fulfilled" && r.value !== priority);
  if (downgraded) toast("error", "实时优先级需要管理员权限，已改为“高”。");
  else if (failed.length > 0)
    toast(
      "error",
      `已更改 ${targets.length - failed.length} 个进程的优先级，${failed.length} 个没有权限或已退出。`,
    );
  else toast("success", `${row.label} 的优先级已设为“${PRIORITY_LABEL[priority]}”。`, 2500);
}

/** Turns efficiency mode on or off for a process, or for every process in an app group. */
export async function setRowEfficiency(row: DisplayRow, on: boolean) {
  const targets = targetsOf(row);
  const results = await Promise.allSettled(targets.map((p) => api.setEfficiency(p.pid, p.createTime, on)));
  const failed = results.filter((r) => r.status === "rejected") as PromiseRejectedResult[];
  const verb = on ? "开启" : "关闭";
  if (failed.length === targets.length)
    toastActionError(failed[0].reason, `无法为 ${row.label} ${verb}效率模式`);
  else if (failed.length > 0)
    toast(
      "error",
      `已为 ${targets.length - failed.length} 个进程${verb}效率模式，${failed.length} 个没有权限或已退出。`,
    );
  else toast("success", `已为 ${row.label} ${verb}效率模式。`, 2500);
}

/**
 * Opens the settings page on the rule for this row's program, creating one
 * that keeps the process's current priority and efficiency mode.
 */
export async function addRuleFor(row: DisplayRow) {
  const name = row.proc.name;
  try {
    const settings = await api.settings();
    let rule = settings.rules.find((r) => r.name.toLowerCase() === name.toLowerCase());
    if (!rule) {
      const priority = priorityOf(row.proc.basePriority);
      const created: ProcessRule = {
        id: crypto.randomUUID(),
        name,
        enabled: true,
        priority: priority && priority !== "normal" ? priority : null,
        efficiency: row.efficiency > 0 ? true : null,
      };
      await api.setRules([...settings.rules, created]);
      rule = created;
    }
    setFocusRule(rule.id);
    setPage("settings");
  } catch (e) {
    toast("error", `无法添加规则：${String(e)}`);
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
export async function endProcessByPid(pid: number, name: string, createTime?: number) {
  const proc = snapshot()?.processes.find(
    (p) => p.pid === pid && (createTime === undefined || p.createTime === createTime),
  );
  const identity = createTime ?? proc?.createTime;
  if (!identity) {
    toast("error", "进程实例信息已失效，请刷新后重试。");
    return false;
  }
  const label = proc?.name ?? name;
  const ok = await confirm({
    title: `结束 ${label}？`,
    body: `PID ${pid}。未保存的数据会丢失。`,
    confirmLabel: "结束进程",
    danger: true,
  });
  if (!ok) return false;
  try {
    await api.kill(pid, identity);
    toast("success", `已结束 ${label}。`, 2500);
    return true;
  } catch (e) {
    toast("error", errorText(e, label));
    return false;
  }
}
