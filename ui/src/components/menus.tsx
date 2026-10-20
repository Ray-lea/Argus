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
import { restartExplorer, restartingExplorer } from "../lib/explorer";
import { t } from "../lib/i18n";
import { api } from "../lib/ipc";
import { applicationMembers } from "../lib/process-identity";
import type { DisplayRow } from "../lib/rows";
import { setDetailsOpen, snapshot, toggleRow } from "../lib/store";
import type { PriorityClass, ProcessIdentity } from "../lib/types";
import { type MenuEntry, menu, openMenu, setMenu } from "../lib/ui";
import {
  BranchEndIcon,
  CopyIcon,
  EndIcon,
  FolderIcon,
  GaugeIcon,
  InfoIcon,
  LeafIcon,
  RestartIcon,
} from "./icons";

/** Task Manager's order, highest first. */
const PRIORITIES: PriorityClass[] = ["realtime", "high", "aboveNormal", "normal", "belowNormal", "idle"];

/** The priority every process of the row shares, or null if they differ. */
function currentPriority(row: DisplayRow): PriorityClass | null {
  if (row.kind !== "group") return priorityOf(row.proc.basePriority);
  const classes = new Set(
    applicationMembers(snapshot()?.processes ?? [], row.key).map((p) => p.basePriority),
  );
  return classes.size === 1 ? priorityOf([...classes][0]) : null;
}

export function processMenu(row: DisplayRow, shell?: ProcessIdentity): MenuEntry[] {
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
  if (shell)
    items.push({
      label: restartingExplorer() ? t("tasks.explorer.restarting") : t("tasks.explorer.restart"),
      icon: () => <RestartIcon />,
      disabled: restartingExplorer(),
      run: () => restartExplorer(shell),
    });
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

/** Resolve the desktop owner only when its menu opens, without adding work to sampling. */
export function openProcessMenu(event: MouseEvent, row: DisplayRow) {
  const items = processMenu(row);
  openMenu(event, items);
  const members =
    row.kind === "group" ? applicationMembers(snapshot()?.processes ?? [], row.key) : [row.proc];
  if (!members.some((p) => p.name.toLowerCase() === "explorer.exe")) return;
  void api
    .desktopShell()
    .then((shell) => {
      if (
        !shell ||
        menu()?.items !== items ||
        !members.some((p) => p.pid === shell.pid && p.createTime === shell.createTime)
      )
        return;
      setMenu((current) =>
        current?.items === items ? { ...current, items: processMenu(row, shell) } : current,
      );
    })
    .catch(() => {
      /* The ordinary process actions remain available. */
    });
}
