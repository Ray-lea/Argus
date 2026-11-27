// Which files each part of `pnpm docs:lint` checks (docs/development/docs-style.md#检查工具).
// `globs` and `ignores` in .markdownlint-cli2.jsonc repeat the first lists for a
// bare markdownlint-cli2 run, and .autocorrectignore the AutoCorrect list; the
// script tests hold them to these.
import { matchesAny } from "./paths.mjs";

/** Every rule: the documentation pages. */
export const PAGES = ["docs/**/*.md"];
/** Structure and link rules only (overrides in .markdownlint-cli2.jsonc): public Markdown outside docs/. */
export const PUBLIC = ["README*.md", "PRIVACY.md", ".github/**/*.md"];
/** Checked by neither markdownlint nor AutoCorrect; their references to docs/ still are. */
export const EXCLUDED = ["**/AGENTS.md", "changelog/**", "vendor/**", "**/node_modules/**", "target/**", "website/content/**"];
/** AutoCorrect: the Chinese documents (the other README translations and the changelog are left out for now). */
export const AUTOCORRECT = ["docs/**/*.md", "README.md", "README.zh-TW.md", "PRIVACY.md", ".github/**/*.md"];
/**
 * Not searched for references to docs/: vendored code, lock files, the
 * generated notices, images, and the script tests, whose fixtures name pages
 * that do not exist.
 */
export const REFERENCE_EXCLUDED = ["vendor/**", "**/*.lock", "pnpm-lock.yaml", "THIRD-PARTY-NOTICES.txt", "docs/images/**", "scripts/tests/**"];

export const isMarkdownlintTarget = (path) => matchesAny(path, [...PAGES, ...PUBLIC]) && !matchesAny(path, EXCLUDED);
export const isAutocorrectTarget = (path) => matchesAny(path, AUTOCORRECT) && !matchesAny(path, EXCLUDED);
export const isReferenceSource = (path) => !matchesAny(path, REFERENCE_EXCLUDED);

/**
 * The files each tool checks, from repository paths: everything in scope
 * when `named` is null, otherwise the named files that are in scope.
 * `outside` lists the named files no tool checks.
 */
export function targets(all, named = null) {
  const pool = named ?? all;
  const markdownlint = pool.filter(isMarkdownlintTarget);
  const autocorrect = pool.filter(isAutocorrectTarget);
  const outside = (named ?? []).filter((path) => !isMarkdownlintTarget(path) && !isAutocorrectTarget(path));
  return { markdownlint, autocorrect, outside };
}
