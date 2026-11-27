// How `pnpm docs:lint` runs AutoCorrect: the CLI of autocorrect-node with the
// repository's .autocorrectrc, in its default diff output (the JSON outputs
// exit with 0 even when they report errors). AutoCorrect skips fenced code
// only when the fence starts its line; inside a list item it would read the
// code as text, and --fix would rewrite it. So it works on copies whose fenced
// code lines are emptied, under the same relative paths (its messages name
// the repository's files and lines), and --fix copies back every other line.
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

// The prebuilt bindings autocorrect-node publishes (its optionalDependencies).
const PREBUILT = ["win32-x64", "darwin-x64", "darwin-arm64", "linux-x64"];

/** Whether autocorrect-node has a binding for this machine (none for Windows or Linux on ARM64). */
export const autocorrectSupported = (platform = process.platform, arch = process.arch) => PREBUILT.includes(`${platform}-${arch}`);

/** The node arguments that lint, or with `fix` rewrite, `files`. */
export function autocorrectArgs(files, { fix = false } = {}) {
  const cli = createRequire(import.meta.url).resolve("autocorrect-node/cli.js");
  return [cli, fix ? "--fix" : "--lint", ...files];
}

/** Markdown with the lines of fenced code emptied, at any indentation, and the indexes of those lines. */
export function maskFences(markdown) {
  const lines = markdown.split("\n");
  const masked = new Set();
  let fence = null;
  for (let i = 0; i < lines.length; i++) {
    const marks = /^[ \t>]*(`{3,}|~{3,})/.exec(lines[i]);
    if (fence) {
      masked.add(i);
      if (marks && marks[1][0] === fence[0] && marks[1].length >= fence.length && !/\S/.test(lines[i].slice(marks[0].length))) fence = null;
    } else if (marks) {
      fence = marks[1];
      masked.add(i);
    }
  }
  return { text: lines.map((line, i) => (masked.has(i) ? (line.endsWith("\r") ? "\r" : "") : line)).join("\n"), masked };
}

/** `fixed` with the masked lines taken back from `original`; null when the lines no longer correspond. */
export function unmask(original, fixed, masked) {
  const before = original.split("\n");
  const after = fixed.split("\n");
  if (before.length !== after.length) return null;
  return after.map((line, i) => (masked.has(i) ? before[i] : line)).join("\n");
}

/**
 * Runs AutoCorrect on `files` (repository paths under `root`) and returns its
 * exit status; with `fix`, writes the corrections back to the files.
 */
export function runAutocorrect(files, { root, fix = false }) {
  const work = mkdtempSync(join(tmpdir(), "argus-docs-lint-"));
  try {
    copyFileSync(join(root, ".autocorrectrc"), join(work, ".autocorrectrc"));
    const copies = new Map();
    for (const file of files) {
      const original = readFileSync(join(root, file), "utf8");
      const { text, masked } = maskFences(original);
      copies.set(file, { original, masked });
      mkdirSync(dirname(join(work, file)), { recursive: true });
      writeFileSync(join(work, file), text);
    }
    const result = spawnSync(process.execPath, autocorrectArgs(files, { fix }), { cwd: work, stdio: "inherit", windowsHide: true });
    if (fix)
      for (const [file, { original, masked }] of copies) {
        const merged = unmask(original, readFileSync(join(work, file), "utf8"), masked);
        if (merged !== null && merged !== original) writeFileSync(join(root, file), merged);
      }
    return result.status ?? 1;
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}
