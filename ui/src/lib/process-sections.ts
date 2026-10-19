export type ProcessSection = "apps" | "background" | "windows";

const SECTIONS: readonly ProcessSection[] = ["apps", "background", "windows"];

/** Expanded members follow their top-level row, even when a child owns no window. */
export function processSections<T extends { depth: number; app: boolean; windows: boolean }>(
  rows: readonly T[],
) {
  const empty = () => ({ rows: [] as T[], count: 0 });
  const groups = { apps: empty(), background: empty(), windows: empty() };
  let group = groups.background;
  for (const row of rows) {
    if (row.depth === 0) {
      group = groups[row.app ? "apps" : row.windows ? "windows" : "background"];
      group.count++;
    }
    group.rows.push(row);
  }
  return SECTIONS.map((section) => ({ section, ...groups[section] })).filter((g) => g.count > 0);
}
