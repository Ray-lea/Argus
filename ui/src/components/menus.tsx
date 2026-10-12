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
import { t } from "../lib/i18n";
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
      label: isGroup ? t("processes.menu.endAll", { count: row.count }) : t("common.endProcess"),
      icon: () => <EndIcon />,
      hint: "Del",
      danger: true,
      run: () => endRow(row),
    },
  ];
  if (!isGroup)
    items.push({ label: t("common.endProcessTree"), icon: () => <BranchEndIcon />, run: () => endTree(row) });
  items.push(
    "separator",
    {
      label: t("processes.menu.setPriority"),
      icon: () => <GaugeIcon />,
      children: PRIORITIES.map((p) => ({
        label: PRIORITY_LABEL[p],
        checked: priority === p,
        run: () => setRowPriority(row, p),
      })),
    },
    {
      label: t("common.efficiencyMode"),
      icon: () => <LeafIcon />,
      checked: efficient,
      run: () => setRowEfficiency(row, !efficient),
    },
    { label: t("processes.menu.addRule"), run: () => addRuleFor(row) },
    "separator",
  );
  if (row.expandable)
    items.push({
      label: row.expanded ? t("common.collapse") : t("common.expand"),
      run: () => toggleRow(row),
    });
  items.push(
    {
      label: t("common.openFileLocation"),
      icon: () => <FolderIcon />,
      disabled: !path,
      run: () => path && revealInExplorer(path),
    },
    {
      label: t("common.copyPath"),
      icon: () => <CopyIcon />,
      disabled: !path,
      run: () => path && copyText(path, t("actions.copyWhat.path")),
    },
    isGroup
      ? { label: t("common.copyName"), run: () => copyText(row.proc.name, t("actions.copyWhat.name")) }
      : {
          label: t("processes.menu.copyPid"),
          run: () => copyText(String(row.proc.pid), t("processes.menu.copyPidWhat")),
        },
    "separator",
    {
      label: t("processes.menu.details"),
      icon: () => <InfoIcon />,
      hint: "Enter",
      run: () => setDetailsOpen(true),
    },
  );
  return items;
}
