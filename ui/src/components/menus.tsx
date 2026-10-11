// Context menu contents, shared by tables and panels.

import { copyText, endRow, endTree, revealInExplorer } from "../lib/actions";
import type { DisplayRow } from "../lib/rows";
import { setDetailsOpen, toggleRow } from "../lib/store";
import type { MenuEntry } from "../lib/ui";
import { BranchEndIcon, CopyIcon, EndIcon, FolderIcon, InfoIcon } from "./icons";

export function processMenu(row: DisplayRow): MenuEntry[] {
  const isGroup = row.kind === "group";
  const path = row.proc.path;
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
  items.push("separator");
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
