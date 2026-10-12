// Context menu contents, shared by tables and panels.

import {
  addRuleFor,
  copyText,
  endRow,
  endTree,
  PRIORITY_LABEL,
  priorityOf,
  revealInExplorer,
  setRowEfficiency,
  setRowPriority,
} from "../lib/actions";
import type { DisplayRow } from "../lib/rows";
import { setDetailsOpen, snapshot, toggleRow } from "../lib/store";
import type { PriorityClass } from "../lib/types";
import type { MenuEntry } from "../lib/ui";
import { BranchEndIcon, CopyIcon, EndIcon, FolderIcon, GaugeIcon, InfoIcon, LeafIcon } from "./icons";

/** Task Manager's order, highest first. */
const PRIORITIES: PriorityClass[] = ["realtime", "high", "aboveNormal", "normal", "belowNormal", "idle"];

/** The priority every process of the row shares, or null if they differ. */
function currentPriority(row: DisplayRow): PriorityClass | null {
  if (row.kind !== "group") return priorityOf(row.proc.basePriority);
  const name = row.proc.name.toLowerCase();
  const classes = new Set(
    (snapshot()?.processes ?? []).filter((p) => p.name.toLowerCase() === name).map((p) => p.basePriority),
  );
  return classes.size === 1 ? priorityOf([...classes][0]) : null;
}

export function processMenu(row: DisplayRow): MenuEntry[] {
  const isGroup = row.kind === "group";
  const path = row.proc.path;
  const priority = currentPriority(row);
  const efficient = row.efficiency > 0 && row.efficiency === row.count;
  const items: MenuEntry[] = [
    {
      label: isGroup ? `结束全部 ${row.count} 个进程` : "结束进程",
      icon: () => <EndIcon />,
      hint: "Del",
      danger: true,
      run: () => endRow(row),
    },
  ];
  if (!isGroup) items.push({ label: "结束进程树", icon: () => <BranchEndIcon />, run: () => endTree(row) });
  items.push(
    "separator",
    {
      label: "设置优先级",
      icon: () => <GaugeIcon />,
      children: PRIORITIES.map((p) => ({
        label: PRIORITY_LABEL[p],
        checked: priority === p,
        run: () => setRowPriority(row, p),
      })),
    },
    {
      label: "效率模式",
      icon: () => <LeafIcon />,
      checked: efficient,
      run: () => setRowEfficiency(row, !efficient),
    },
    { label: "添加规则", run: () => addRuleFor(row) },
    "separator",
  );
  if (row.expandable) items.push({ label: row.expanded ? "折叠" : "展开", run: () => toggleRow(row) });
  items.push(
    {
      label: "打开文件位置",
      icon: () => <FolderIcon />,
      disabled: !path,
      run: () => path && revealInExplorer(path),
    },
    {
      label: "复制路径",
      icon: () => <CopyIcon />,
      disabled: !path,
      run: () => path && copyText(path, "路径"),
    },
    isGroup
      ? { label: "复制名称", run: () => copyText(row.proc.name, "名称") }
      : { label: "复制 PID", run: () => copyText(String(row.proc.pid), " PID") },
    "separator",
    { label: "详细信息", icon: () => <InfoIcon />, hint: "Enter", run: () => setDetailsOpen(true) },
  );
  return items;
}
