// Notes and pins on the rows of the services, startup and connections pages:
// what a mark belongs to, how it is stored, and pinned rows first.
// No Solid; run by ui/tests directly.

import type { Connection } from "./types.ts";

/** A row's note, its pin, or both; a row with neither has no entry. */
export interface RowMark {
  note?: string;
  pinned?: true;
}

export type RowMarkMap = Readonly<Record<string, RowMark>>;

/** Longest note kept, in characters. */
export const NOTE_MAX = 80;

/** One line, trimmed and at most `NOTE_MAX` characters; empty for no note. */
export function cleanNote(text: string): string {
  const chars = [...text.replace(/\s+/g, " ").trim()];
  return chars.length > NOTE_MAX ? chars.slice(0, NOTE_MAX).join("").trimEnd() : chars.join("");
}

/** `marks` with `key`'s note or pin changed; a key left with neither is dropped. */
export function withMark(
  marks: RowMarkMap,
  key: string,
  change: { note?: string; pinned?: boolean },
): Record<string, RowMark> {
  const mark: RowMark = { ...marks[key] };
  if (change.note !== undefined) {
    const note = cleanNote(change.note);
    if (note) mark.note = note;
    else delete mark.note;
  }
  if (change.pinned !== undefined) {
    if (change.pinned) mark.pinned = true;
    else delete mark.pinned;
  }
  const next = { ...marks };
  if (mark.note || mark.pinned) next[key] = mark;
  else delete next[key];
  return next;
}

/** Pinned rows first, both groups keeping their order. */
export function pinnedFirst<T>(rows: readonly T[], pinned: (row: T) => boolean): readonly T[] {
  const top: T[] = [];
  const rest: T[] = [];
  for (const row of rows) (pinned(row) ? top : rest).push(row);
  return top.length ? [...top, ...rest] : rows;
}

type Endpoint = Pick<Connection, "protocol" | "pid" | "localPort" | "remoteAddress" | "remotePort" | "state">;

/** The local ports each process listens on (TCP), as `pid|port`. */
export function listeningPorts(conns: readonly Endpoint[]): Set<string> {
  const ports = new Set<string>();
  for (const c of conns) if (c.state === "listen") ports.add(`${c.pid}|${c.localPort}`);
  return ports;
}

/**
 * Which end names a connection for its note and pin: the local port for a
 * listening port, a UDP port and the connections accepted on a listening port;
 * the remote endpoint for connections the program opened, whose local port
 * changes every time.
 */
export const connectionSide = (c: Endpoint, listening: ReadonlySet<string>): "local" | "remote" =>
  c.remoteAddress === null || listening.has(`${c.pid}|${c.localPort}`) ? "local" : "remote";

/**
 * What a connection's note and pin belong to: the program and the end
 * `connectionSide` picks, so they outlive the connection and the program's
 * restarts; null while the program is unknown.
 */
export function connectionMark(
  c: Endpoint,
  program: string | undefined,
  listening: ReadonlySet<string>,
): string | null {
  if (!program) return null;
  const name = program.toLowerCase();
  return connectionSide(c, listening) === "local"
    ? `${c.protocol}|${name}|local|${c.localPort}`
    : `${c.protocol}|${name}|remote|${c.remoteAddress}|${c.remotePort ?? ""}`;
}
