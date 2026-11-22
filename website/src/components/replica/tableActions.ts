import type { MenuEntry } from "./Pages";

function cellText(cell: HTMLElement) {
  const label = cell.querySelector<HTMLElement>(".a-th-label,.a-name,.a-group-name");
  return (label ?? cell).innerText.trim();
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

export function exportTable(page: HTMLElement, name: string) {
  const grid = page.querySelector<HTMLElement>('[role="grid"],.a-ptable');
  if (!grid) return;
  const fields = columns(grid);
  const rows = [...grid.querySelectorAll<HTMLElement>(".a-tbody > [role=row]")];
  const records = [
    fields.map((c) => c.label),
    ...rows.map((row) => {
      const cells = [...row.querySelectorAll<HTMLElement>(":scope > .a-td")];
      return fields.map(({ index }) => (cells[index] ? cellText(cells[index]) : ""));
    }),
  ];
  const escape = (value: string) => {
    const safe = /^[\s]*[=+@-]/.test(value) ? "'" + value : value;
    return '"' + safe.replaceAll('"', '""') + '"';
  };
  const url = URL.createObjectURL(
    new Blob(["\uFEFF", records.map((row) => row.map(escape).join(",")).join("\r\n"), "\r\n"], {
      type: "text/csv;charset=utf-8",
    }),
  );
  const link = document.createElement("a");
  link.href = url;
  link.download = `Argus-${name}-${new Date().toISOString().slice(0, 10)}.csv`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
