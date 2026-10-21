// Only the application's script locations and fixed error classifications
// cross IPC. Exception messages, arbitrary stack text and URL queries do not.
export interface ErrorFrame {
  file: string;
  line: number;
  column: number;
}

const names = new Set([
  "TypeError",
  "RangeError",
  "ReferenceError",
  "SyntaxError",
  "URIError",
  "EvalError",
  "AbortError",
  "NetworkError",
  "Error",
]);
const kinds = new Set(["accessDenied", "notFound", "critical", "refused", "serviceOutdated", "os"]);

export class ErrorGate {
  private seen = new Map<string, number>();
  admit(key: string, now: number): boolean {
    const previous = this.seen.get(key);
    if (previous !== undefined && now - previous < 60_000) return false;
    if (!this.seen.has(key) && this.seen.size >= 64)
      this.seen.delete(this.seen.keys().next().value as string);
    this.seen.set(key, now);
    return true;
  }
}

function frame(url: string, line: number, column: number, origin: string): ErrorFrame | null {
  if (
    !Number.isInteger(line) ||
    line <= 0 ||
    line > 1_000_000 ||
    !Number.isInteger(column) ||
    column < 0 ||
    column > 10_000_000
  )
    return null;
  try {
    const parsed = new URL(url);
    if (parsed.origin !== origin || parsed.username || parsed.password) return null;
    const file = decodeURIComponent(parsed.pathname).replace(/^\//, "");
    if (file.length > 180 || !/^(?:assets\/[\w.-]+\.js|src\/(?:[\w-]+\/)*[\w.-]+\.tsx?)$/.test(file))
      return null;
    return { file, line, column };
  } catch {
    return null;
  }
}

function nativeCodeOf(value: unknown): number | null {
  if (typeof value !== "string") return null;
  const text = value.slice(0, 4096);
  const numeric = /(?:^|[\s(])os error (\d+)\b/.exec(text);
  const hex = /(?:^|[^\w])0x([89a-f][\da-f]{7})(?![\da-f])/i.exec(text);
  const code = numeric ? Number(numeric[1]) : hex ? Number.parseInt(hex[1], 16) : null;
  return code !== null && Number.isInteger(code) && code >= 0 && code <= 0xffff_ffff ? code : null;
}

export function errorReport(
  error: unknown,
  origin: string,
  location?: { url: string; line: number; column: number },
) {
  let category = "Unknown";
  let nativeCode: number | null = null;
  let actionKind: string | null = null;
  const frames: ErrorFrame[] = [];
  const add = (item: ErrorFrame | null) => {
    if (
      item &&
      frames.length < 5 &&
      !frames.some((f) => f.file === item.file && f.line === item.line && f.column === item.column)
    )
      frames.push(item);
  };
  if (location) add(frame(location.url, location.line, location.column, origin));
  try {
    if (error && typeof error === "object") {
      const value = error as {
        name?: unknown;
        stack?: unknown;
        code?: unknown;
        kind?: unknown;
        message?: unknown;
      };
      if (typeof value.name === "string" && names.has(value.name)) category = value.name;
      if (
        typeof value.code === "number" &&
        Number.isInteger(value.code) &&
        value.code >= 0 &&
        value.code <= 0xffff_ffff
      )
        nativeCode = value.code;
      if (typeof value.kind === "string" && kinds.has(value.kind)) actionKind = value.kind;
      if (nativeCode === null) nativeCode = nativeCodeOf(value.message);
      if (typeof value.stack === "string") {
        for (const match of value.stack
          .slice(0, 16_384)
          .matchAll(/((?:https?|tauri):\/\/[^\s)]+):(\d+):(\d+)/g)) {
          add(frame(match[1], Number(match[2]), Number(match[3]), origin));
        }
      }
    } else if (typeof error === "string") {
      nativeCode = nativeCodeOf(error);
    }
  } catch {
    // A rejection can even be a proxy whose getters throw.
  }
  return { category, nativeCode, actionKind, valueType: error === null ? "null" : typeof error, frames };
}
