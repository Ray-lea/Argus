// Where a link inside the repository leads, and whether the file and the
// anchor are there: shared by the argus-links rule and the search for
// references to docs/ in the other files.
import { dirname, resolve } from "node:path";
import { anchorsOfFile, decodeFragment, isLineFragment } from "./anchors.mjs";
import { REPO_ROOT, existence, isFolder, repoPath } from "./paths.mjs";

/** Links that leave the repository: a scheme (https:, mailto:) or a host (//host). */
export const isExternal = (destination) => /^[a-z][a-z0-9+.-]*:/i.test(destination) || destination.startsWith("//");

/**
 * The file and fragment a relative link from `from` (an absolute path) leads
 * to: { file, fragment } with `file` absolute, `fragment` null without a #.
 * A link that starts with / is relative to `root`, as on GitHub. Null for
 * external links and for fragments of the page itself.
 */
export function resolveLink(destination, from, root = REPO_ROOT) {
  if (isExternal(destination)) return null;
  const hash = destination.indexOf("#");
  const fragment = hash === -1 ? null : destination.slice(hash + 1);
  const path = (hash === -1 ? destination : destination.slice(0, hash)).replace(/\?.*$/, "");
  if (!path) return null;
  let decoded = path;
  try {
    decoded = decodeURIComponent(path);
  } catch {
    // A stray % stays as written; the file will not be found.
  }
  const file = decoded.startsWith("/") ? resolve(root, `.${decoded}`) : resolve(dirname(from), decoded);
  return { file, fragment };
}

/**
 * What is wrong with a link target, or null: { problem: "missing" | "case" |
 * "anchor", path, fragment }. Anchors are checked on Markdown pages only, and
 * GitHub's line fragments (#L10) are left alone.
 */
export function checkTarget({ file, fragment }, root = REPO_ROOT) {
  const path = repoPath(file, root);
  const found = existence(file, root);
  if (found !== "yes") return { problem: found === "case" ? "case" : "missing", path, fragment };
  if (fragment === null || fragment === "" || isLineFragment(fragment) || !/\.md$/i.test(file) || isFolder(file)) return null;
  const decoded = decodeFragment(fragment);
  return anchorsOfFile(file).has(decoded) ? null : { problem: "anchor", path, fragment: decoded };
}

/** The message for a problem of `checkTarget`, by the console language. */
export function describe(problem, t) {
  const target = problem.fragment === null || problem.problem !== "anchor" ? problem.path : `${problem.path}#${problem.fragment}`;
  if (problem.problem === "missing") return t(`目标不存在：${target}`, `no such file: ${target}`);
  if (problem.problem === "case") return t(`目标的大小写与文件名不一致（GitHub 区分大小写）：${target}`, `the case differs from the file's name (GitHub is case-sensitive): ${target}`);
  return t(`锚点不存在：${target}`, `no such anchor: ${target}`);
}
