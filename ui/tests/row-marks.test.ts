// Notes and pins on table rows: what they belong to and how pinned rows sort.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  cleanNote,
  connectionMark,
  listeningPorts,
  NOTE_MAX,
  pinnedFirst,
  withMark,
} from "../src/lib/row-marks.ts";
import type { Connection } from "../src/lib/types.ts";

test("a note is one trimmed line of at most NOTE_MAX characters", () => {
  assert.equal(cleanNote("  公司\n VPN\t客户端  "), "公司 VPN 客户端");
  assert.equal(cleanNote(" \n "), "");
  const long = "备".repeat(NOTE_MAX + 5);
  assert.equal([...cleanNote(long)].length, NOTE_MAX);
  // Characters, not UTF-16 units: an emoji is not cut in half.
  const emoji = "😀".repeat(NOTE_MAX + 1);
  assert.equal(cleanNote(emoji), "😀".repeat(NOTE_MAX));
});

test("notes and pins are kept together and dropped together", () => {
  let marks = withMark({}, "wuauserv", { note: " 更新 " });
  assert.deepEqual(marks, { wuauserv: { note: "更新" } });
  marks = withMark(marks, "wuauserv", { pinned: true });
  assert.deepEqual(marks, { wuauserv: { note: "更新", pinned: true } });
  // Removing the note keeps the pin; removing both drops the entry.
  marks = withMark(marks, "wuauserv", { note: "  " });
  assert.deepEqual(marks, { wuauserv: { pinned: true } });
  marks = withMark(marks, "wuauserv", { pinned: false });
  assert.deepEqual(marks, {});
  // The map passed in is left as it was.
  const before = { a: { note: "x" } };
  withMark(before, "a", { note: "" });
  assert.deepEqual(before, { a: { note: "x" } });
});

test("pinned rows come first, both groups keeping their order", () => {
  const rows = ["a", "B", "c", "D", "e"];
  const upper = (r: string) => r === r.toUpperCase();
  assert.deepEqual(pinnedFirst(rows, upper), ["B", "D", "a", "c", "e"]);
  // Nothing pinned: the same list back.
  assert.equal(
    pinnedFirst(rows, () => false),
    rows,
  );
});

const conn = (c: Partial<Connection>): Connection => ({
  protocol: "tcp",
  ipv6: false,
  localAddress: "127.0.0.1",
  localPort: 50000,
  remoteAddress: "140.82.112.3",
  remotePort: 443,
  state: "established",
  pid: 100,
  ...c,
});

test("a connection's mark follows the program and its lasting end", () => {
  const listener = conn({
    localAddress: "0.0.0.0",
    localPort: 5173,
    remoteAddress: null,
    remotePort: null,
    state: "listen",
  });
  const accepted = conn({ localPort: 5173, remoteAddress: "127.0.0.1", remotePort: 61234 });
  const opened = conn({ pid: 200, localPort: 61234, remoteAddress: "127.0.0.1", remotePort: 5173 });
  const listening = listeningPorts([listener, accepted, opened]);

  // The listener and the connections it accepted share the local port.
  const local = connectionMark(listener, "Node.exe", listening);
  assert.equal(local, "tcp|node.exe|local|5173");
  assert.equal(connectionMark(accepted, "node.exe", listening), local);
  // Its other ports and other programs on the same port are different.
  assert.equal(
    connectionMark({ ...listener, localPort: 3000 }, "node.exe", listening),
    "tcp|node.exe|local|3000",
  );
  assert.equal(connectionMark(listener, "python.exe", listening), "tcp|python.exe|local|5173");

  // An opened connection goes by where it leads, not its changing local port.
  const remote = connectionMark(opened, "chrome.exe", listening);
  assert.equal(remote, "tcp|chrome.exe|remote|127.0.0.1|5173");
  assert.equal(connectionMark({ ...opened, localPort: 61300 }, "chrome.exe", listening), remote);

  // UDP has no remote end; an unknown program has no mark.
  const udp = conn({ protocol: "udp", localPort: 5353, remoteAddress: null, remotePort: null, state: null });
  assert.equal(connectionMark(udp, "svchost.exe", listening), "udp|svchost.exe|local|5353");
  assert.equal(connectionMark(opened, undefined, listening), null);
});
