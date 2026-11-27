// Paths shared by the documentation checks: the repository root, repository
// paths with forward slashes, the small glob syntax the rule parameters use
// (`**` any number of folders, `*` and `?` within one name), and existence
// checks that respect the case of names, as GitHub does on any file system.
import { existsSync, readdirSync, statSync } from "node:fs";
import { dirname, isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

export const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");

/** `file` (absolute, or relative to `root`) as a path inside `root`, with forward slashes. */
export function repoPath(file, root = REPO_ROOT) {
  return relative(root, isAbsolute(file) ? file : resolve(root, file)).split(sep).join("/");
}

const globs = new Map();

/** The regular expression of one glob. */
export function globToRegExp(glob) {
  let compiled = globs.get(glob);
  if (compiled) return compiled;
  let source = "";
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === "*" && glob[i + 1] === "*") {
      // "**/" matches no folder or any number of them; a final "**" anything.
      if (glob[i + 2] === "/") {
        source += "(?:.*/)?";
        i += 2;
      } else {
        source += ".*";
        i += 1;
      }
    } else if (c === "*") source += "[^/]*";
    else if (c === "?") source += "[^/]";
    else source += c.replace(/[.+^${}()|[\]\\]/g, "\\$&");
  }
  compiled = new RegExp(`^${source}$`);
  globs.set(glob, compiled);
  return compiled;
}

/** Whether a repository path matches any of `patterns`. */
export const matchesAny = (path, patterns) => patterns.some((pattern) => globToRegExp(pattern).test(path));

const listings = new Map();

function names(folder) {
  let list = listings.get(folder);
  if (!list) {
    try {
      list = readdirSync(folder);
    } catch {
      list = [];
    }
    listings.set(folder, list);
  }
  return list;
}

/** Forget the folder listings (tests create files between checks). */
export function clearListings() {
  listings.clear();
}

/**
 * Whether `file` exists: "yes", "no", or "case" when it exists only with
 * different capitals (Windows and macOS find it, GitHub does not). The case of
 * names is checked below `root`; elsewhere only existence.
 */
export function existence(file, root = REPO_ROOT) {
  const inside = relative(root, file);
  if (!inside) return "yes";
  if (inside.startsWith("..") || isAbsolute(inside)) return existsSync(file) ? "yes" : "no";
  let folder = root;
  for (const name of inside.split(sep)) {
    if (!name) continue;
    const listed = names(folder);
    if (!listed.includes(name)) return listed.some((other) => other.toLowerCase() === name.toLowerCase()) ? "case" : "no";
    folder = resolve(folder, name);
  }
  return "yes";
}

/** Whether `file` is a folder. */
export function isFolder(file) {
  try {
    return statSync(file).isDirectory();
  } catch {
    return false;
  }
}
