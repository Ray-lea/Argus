import type { MenuEntry } from "./Pages";
import { translate } from "./i18n";

// A cell's text as the table shows it, without the marks beside it (a tag, a
// count); copied, a cell can say more than it shows (`data-copy`: TCPv6).
function cellText(cell: HTMLElement, copying = false) {
  if (copying && cell.dataset.copy !== undefined) return cell.dataset.copy;
  const label = cell.querySelector<HTMLElement>(".a-th-label,.a-name,.a-group-name");
  if (label) return label.innerText.trim();
  let text = cell.innerText;
  for (const mark of cell.querySelectorAll<HTMLElement>(".a-tag,.a-count,.a-badge")) text = text.replace(mark.innerText, "");
  return text.trim();
}

function columns(grid: HTMLElement) {
  return [...grid.querySelectorAll<HTMLElement>('[role="columnheader"]')]
    .map((cell, index) => ({
      index,
      label: cellText(cell),
      visible: cell.offsetWidth > 0,
    }))
    .filter((col) => col.visible);
}

export function copyEntries(target: Element, name: string, extra: MenuEntry[] = []): MenuEntry[] {
  const row = target.closest<HTMLElement>('[role="row"]');
  const grid = row?.closest<HTMLElement>('[role="grid"],.a-ptable');
  if (!row || !grid) return [];
  const fields = columns(grid);
  const cells = [...row.querySelectorAll<HTMLElement>(":scope > .a-td")];
  const column = fields.find((c) => cells[c.index]?.contains(target));
  const get = (i: number) => (cells[i] ? cellText(cells[i]) : "");
  const entry = (label: string, value: string): MenuEntry => ({
    label,
    disabled: !value,
    run: () => void navigator.clipboard?.writeText(value),
  });
  const details = fields
    .filter((c) => get(c.index))
    .map((c) => c.label + ": " + get(c.index))
    .join("\n");
  return [
    { label: "复制", icon: "copy", children: [entry("名称", name), ...extra, entry("详细信息", details)] },
    ...(column ? [entry(`复制“${column.label}”`, get(column.index))] : []),
  ];
}

// The service, startup and connection lists copy as the app's (list_copy.rs):
// a row's name is its first cell, its details every cell shown, one
// "Name：value" a line; empty cells and "—" are left out.
export function rowCopy(row: HTMLElement) {
  const grid = row.closest<HTMLElement>('[role="grid"]');
  const fields = grid ? columns(grid) : [];
  const cells = [...row.querySelectorAll<HTMLElement>(":scope > .a-td")];
  const get = (i: number) => (cells[i] ? cellText(cells[i], true) : "");
  const details = fields
    .filter((c) => get(c.index) && get(c.index) !== "—")
    .map((c) => translate(`${c.label}：${get(c.index)}`))
    .join("\n");
  return { fields, cells, get, name: get(0), first: fields[0]?.label ?? "", details };
}

// One "Copy" submenu, as the app's lists gather it (menus.rs `open_row_menu`):
// the page's own copies, the name (Ctrl+C), the column right-clicked unless it
// is the name, then the details (Ctrl+Shift+C).
export function listCopy(target: Element, own: MenuEntry[], copy: (text: string, what: string) => void): MenuEntry[] {
  const row = target.closest<HTMLElement>('[role="row"]');
  if (!row) return [];
  const { fields, cells, get, name, first, details } = rowCopy(row);
  const column = fields.find((c) => c.index > 0 && cells[c.index]?.contains(target));
  const value = column ? get(column.index) : "";
  return [
    {
      label: "复制",
      icon: "copy",
      children: [
        ...own.map((entry) => (entry === "-" ? entry : { ...entry, icon: undefined })),
        { label: first, hint: "Ctrl+C", run: () => copy(name, "名称") },
        ...(column
          ? [{ label: `复制“${column.label}”`, disabled: !value || value === "—", run: () => copy(value, `“${column.label}”`) }]
          : []),
        "-",
        { label: "详细信息", hint: "Ctrl+Shift+C", run: () => copy(details, "详细信息") },
      ],
    },
  ];
}

const pad = (n: number) => String(n).padStart(2, "0");
// Saves the page's table as the app does (tables/export.rs): every column and
// row shown, a cell quoted only when it holds a comma, quote or line break,
// named "Argus-<page>[-<view>]-<date>_<time>-<rows>.csv". Returns the rows.
export function exportTable(page: HTMLElement, name: string, view?: string) {
  const grid = page.querySelector<HTMLElement>('[role="grid"],.a-ptable');
  if (!grid) return 0;
  const fields = columns(grid);
  const rows = [...grid.querySelectorAll<HTMLElement>(".a-tbody > [role=row]")];
  const records = [
    fields.map((c) => c.label),
    ...rows.map((row) => {
      const cells = [...row.querySelectorAll<HTMLElement>(":scope > .a-td")];
      return fields.map(({ index }) => (cells[index] ? cellText(cells[index]) : ""));
    }),
  ];
  // External text that a spreadsheet would run as a formula starts with a quote.
  const escape = (value: string) => {
    const safe = /^[\t\r\n]|^\s*[=+@-]/.test(value) ? "'" + value : value;
    return /[,"\r\n]/.test(safe) ? '"' + safe.replaceAll('"', '""') + '"' : safe;
  };
  const url = URL.createObjectURL(
    new Blob(["\uFEFF", records.map((row) => row.map(escape).join(",")).join("\r\n"), "\r\n"], {
      type: "text/csv;charset=utf-8",
    }),
  );
  const link = document.createElement("a");
  link.href = url;
  const t = new Date();
  const when = `${t.getFullYear()}-${pad(t.getMonth() + 1)}-${pad(t.getDate())}_${pad(t.getHours())}-${pad(t.getMinutes())}-${pad(t.getSeconds())}`;
  const title = translate(name, "nav") + (view ? "-" + translate(view) : "");
  link.download = `Argus-${title}-${when}-${translate(`${rows.length}行`)}.csv`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  return rows.length;
}
