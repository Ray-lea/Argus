// User-facing process operations: confirm, run, report.

import { actionFailure, endFailure, needsServiceUpdate, telling } from "./action-errors";
import { t } from "./i18n";
import { api } from "./ipc";
import { SERVICE_SETTING } from "./privilege";
import type { DisplayRow } from "./rows";
import { goToSetting, markEnding, setFocusRule, setPage, snapshot } from "./store";
import type { PriorityClass, ProcessRow, ProcessRule } from "./types";
import { confirm, dismissToast, toast } from "./ui";

/** Display names of the priority classes; getters, so each read follows the UI language. */
export const PRIORITY_LABEL: Readonly<Record<PriorityClass, string>> = {
  get realtime() {
    return t("processes.priority.realtime");
  },
  get high() {
    return t("processes.priority.high");
  },
  get aboveNormal() {
    return t("processes.priority.aboveNormal");
  },
  get normal() {
    return t("processes.priority.normal");
  },
  get belowNormal() {
    return t("processes.priority.belowNormal");
  },
  get idle() {
    return t("processes.priority.idle");
  },
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
  const { key, error } = endFailure(e);
  return t(key, error === undefined ? { name } : { name, error });
}

/** The notice offering the service update, while one may still show. */
let updateNotice: number | null = null;

/**
 * Reports a failure; when one of `errors` was denied while the privileged
 * service needs an update, the notice stays longer and leads to its card.
 * Such a notice replaces the previous one: stacked, a few of them reach past
 * the top of a small window, and those left behind cover the card's buttons.
 */
function toastFailure(text: string, errors: readonly unknown[]) {
  if (!needsServiceUpdate(errors)) {
    toast("error", text);
    return;
  }
  if (updateNotice !== null) dismissToast(updateNotice);
  updateNotice = toast("error", text, 10000, {
    actions: [
      {
        label: t("settings.privilege.update"),
        run: () => goToSetting("privilege", SERVICE_SETTING),
      },
    ],
  });
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
        title: t("actions.end.title", { name: row.label }),
        body: t("actions.endGroup.body", { name: row.proc.name, count: members.length }),
        confirmLabel: t("actions.endAll"),
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
    const failed = (results.filter((r) => r.status === "rejected") as PromiseRejectedResult[]).map(
      (r) => r.reason,
    );
    if (failed.length === 0)
      toast("success", t("actions.endGroup.done", { name: row.label, count: members.length }));
    else
      toastFailure(
        t("actions.endGroup.partial", {
          error: errorText(telling(failed), row.label),
          count: failed.length,
        }),
        failed,
      );
    return;
  }

  const p = row.proc;
  if (
    !skipConfirm &&
    !(await confirm({
      title: t("actions.end.title", { name: p.name }),
      body: t("actions.end.body", { pid: p.pid }),
      confirmLabel: t("common.endProcess"),
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
    toast("success", t("actions.end.done", { name: p.name }), 2500);
  } catch (e) {
    toastFailure(errorText(e, p.name), [e]);
  }
}

export async function endTree(row: DisplayRow) {
  const p = row.proc;
  const n = descendantCount(p);
  const ok = await confirm({
    title: t("actions.endTree.title", { name: p.name }),
    body:
      n > 0
        ? t("actions.endTree.body", { pid: p.pid, count: n })
        : t("actions.endTree.bodyAlone", { pid: p.pid }),
    confirmLabel: t("common.endProcessTree"),
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
      // The report keeps no errors, only whether a denial asks for the service's update.
      toastFailure(
        t("actions.endTree.partial", { count: report.terminated, failed: report.failed }),
        report.serviceOutdated ? [{ kind: "serviceOutdated" }] : [],
      );
    } else {
      toast("success", t("actions.endTree.done", { count: report.terminated }), 2500);
    }
  } catch (e) {
    toastFailure(errorText(e, p.name), [e]);
  }
}

/** The processes an action on `row` applies to: one, or every member of an app group. */
const targetsOf = (row: DisplayRow) => (row.kind === "group" ? groupMembers(row) : [row.proc]);

/** Sets the priority class of a process, or of every process in an app group. */
export async function setRowPriority(row: DisplayRow, priority: PriorityClass) {
  if (
    priority === "realtime" &&
    !(await confirm({
      title: t("actions.priority.confirmTitle", { name: row.label }),
      body: t("actions.priority.confirmBody"),
      confirmLabel: t("actions.priority.confirm"),
      danger: true,
    }))
  ) {
    return;
  }
  const targets = targetsOf(row);
  const results = await Promise.allSettled(
    targets.map((p) => api.setPriority(p.pid, p.createTime, priority)),
  );
  const failed = (results.filter((r) => r.status === "rejected") as PromiseRejectedResult[]).map(
    (r) => r.reason,
  );
  if (failed.length === targets.length) {
    toastActionError(telling(failed), t("actions.priority.failed", { name: row.label }));
    return;
  }
  const downgraded = results.some((r) => r.status === "fulfilled" && r.value !== priority);
  if (downgraded) toast("error", t("actions.priority.downgraded"));
  else if (failed.length > 0)
    toastFailure(
      t("actions.priority.partial", { count: targets.length - failed.length, failed: failed.length }),
      failed,
    );
  else
    toast(
      "success",
      t("actions.priority.done", { name: row.label, priority: PRIORITY_LABEL[priority] }),
      2500,
    );
}

/** Turns efficiency mode on or off for a process, or for every process in an app group. */
export async function setRowEfficiency(row: DisplayRow, on: boolean) {
  const targets = targetsOf(row);
  const results = await Promise.allSettled(targets.map((p) => api.setEfficiency(p.pid, p.createTime, on)));
  const failed = (results.filter((r) => r.status === "rejected") as PromiseRejectedResult[]).map(
    (r) => r.reason,
  );
  const state = on ? "on" : "off";
  if (failed.length === targets.length)
    toastActionError(telling(failed), t(`actions.efficiency.${state}.failed`, { name: row.label }));
  else if (failed.length > 0)
    toastFailure(
      t(`actions.efficiency.${state}.partial`, {
        count: targets.length - failed.length,
        failed: failed.length,
      }),
      failed,
    );
  else toast("success", t(`actions.efficiency.${state}.done`, { name: row.label }), 2500);
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
    toast("error", t("actions.addRuleFailed", { error: String(e) }));
  }
}

/** `what` names the copied text in the UI language, e.g. `t("common.path")`. */
export async function copyText(text: string, what: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast("success", t("actions.copied", { what }), 1800);
  } catch {
    toast("error", t("actions.copyFailed", { what }));
  }
}

export async function revealInExplorer(path: string) {
  try {
    await api.reveal(path);
  } catch (e) {
    toast("error", t("actions.revealFailed", { error: String(e) }));
  }
}

/** Reports a failed operation in plain terms, e.g. "无法停止“Print Spooler”：需要管理员权限。" */
export function toastActionError(e: unknown, what: string) {
  const { key, error } = actionFailure(e);
  toastFailure(t(key, error === undefined ? { what } : { what, error }), [e]);
}

/** Ends one process by PID (connections and file locks have no table row to act on). */
export async function endProcessByPid(pid: number, name: string, createTime?: number) {
  const proc = snapshot()?.processes.find(
    (p) => p.pid === pid && (createTime === undefined || p.createTime === createTime),
  );
  const identity = createTime ?? proc?.createTime;
  if (!identity) {
    toast("error", t("actions.staleProcess"));
    return false;
  }
  const label = proc?.name ?? name;
  const ok = await confirm({
    title: t("actions.end.title", { name: label }),
    body: t("actions.end.body", { pid }),
    confirmLabel: t("common.endProcess"),
    danger: true,
  });
  if (!ok) return false;
  try {
    await api.kill(pid, identity);
    toast("success", t("actions.end.done", { name: label }), 2500);
    return true;
  } catch (e) {
    toastFailure(errorText(e, label), [e]);
    return false;
  }
}
