// Notes and pins the user put on rows, kept per page in localStorage.

import { persisted } from "./prefs";
import { type RowMark, withMark } from "./row-marks";

export interface RowMarks {
  note: (key: string) => string | undefined;
  pinned: (key: string) => boolean;
  /** An empty note removes it. */
  setNote: (key: string, note: string) => void;
  setPinned: (key: string, pinned: boolean) => void;
}

export function rowMarks(table: "services" | "startup" | "connections"): RowMarks {
  const [marks, setMarks] = persisted<Record<string, RowMark>>(`rowMarks.${table}`, {});
  return {
    note: (key) => marks()[key]?.note,
    pinned: (key) => marks()[key]?.pinned === true,
    setNote: (key, note) => setMarks(withMark(marks(), key, { note })),
    setPinned: (key, pinned) => setMarks(withMark(marks(), key, { pinned })),
  };
}
