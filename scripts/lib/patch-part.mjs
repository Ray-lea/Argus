// Parts of a one-file `git diff`, for staging some of a file's changes.

/** The diff's header lines and its hunks, each a list of lines starting with its `@@` line. */
export function splitPatch(text) {
  const head = [];
  const hunks = [];
  const lines = text.split("\n");
  if (lines.at(-1) === "") lines.pop();
  for (const line of lines) {
    if (line.startsWith("@@")) hunks.push([line]);
    else if (hunks.length) hunks.at(-1).push(line);
    else head.push(line);
  }
  return { head, hunks };
}

/** One line per hunk: its number, its `@@` line and the first changed lines. */
export function listHunks(text, shown = 3) {
  return splitPatch(text).hunks.map((hunk, index) => {
    const changes = hunk.slice(1).filter((line) => /^[+-]/.test(line));
    const more = changes.length > shown ? `\n     … ${changes.length - shown} more changed lines` : "";
    return `${String(index + 1).padStart(3)}  ${hunk[0]}\n${changes.slice(0, shown).map((line) => `     ${line}`).join("\n")}${more}`;
  });
}

/**
 * The patch holding only the hunks numbered in `keep` (from 1; all of them
 * when `keep` is "all"), without the changed lines that contain one of the
 * `leave` texts: a left-out added line is dropped, a left-out removed line
 * stays as context. A hunk with no change left is dropped. Line counts in the
 * `@@` lines are not corrected: apply with `git apply --recount`.
 */
export function selectPatch(text, keep, leave = []) {
  const { head, hunks } = splitPatch(text);
  if (keep !== "all") for (const number of keep) if (!Number.isInteger(number) || number < 1 || number > hunks.length) throw Error(`no hunk ${number}: the diff has ${hunks.length}`);
  const used = new Set();
  const out = [];
  let kept = 0;
  hunks.forEach((hunk, index) => {
    if (keep !== "all" && !keep.includes(index + 1)) return;
    const body = [];
    let changes = 0;
    for (const line of hunk.slice(1)) {
      const changed = /^[+-]/.test(line);
      const left = changed ? leave.find((part) => line.slice(1).includes(part)) : undefined;
      if (left !== undefined) {
        used.add(left);
        if (line.startsWith("-")) body.push(` ${line.slice(1)}`);
        continue;
      }
      if (changed) changes++;
      body.push(line);
    }
    if (!changes) return;
    out.push(hunk[0], ...body);
    kept++;
  });
  for (const part of leave) if (!used.has(part)) throw Error(`no changed line of the chosen hunks contains "${part}"`);
  return { patch: kept ? `${[...head, ...out].join("\n")}\n` : "", kept };
}
