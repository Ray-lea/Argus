// Macro calls in Rust source, read with balanced brackets and string literals
// skipped, so a call nested in another's arguments is neither mistaken for
// the outer call's arguments nor missed.

const OPEN = { "(": ")", "[": "]", "{": "}" };
const CLOSE = new Set(Object.values(OPEN));

/** Where the string or char literal starting at `at` ends (exclusive). */
function literalEnd(text, at) {
  const quote = text[at];
  for (let i = at + 1; i < text.length; i++) {
    if (text[i] === "\\") i++;
    else if (text[i] === quote) return i + 1;
  }
  return text.length;
}

/** The text up to the bracket closing the one just before `at`, or null. */
function balanced(text, at) {
  let depth = 1;
  for (let i = at; i < text.length; i++) {
    const c = text[i];
    if (c === '"') {
      i = literalEnd(text, i) - 1;
    } else if (c in OPEN) {
      depth++;
    } else if (CLOSE.has(c) && --depth === 0) {
      return text.slice(at, i);
    }
  }
  return null;
}

/** Splits arguments at the commas outside brackets and strings. */
export function topLevelArguments(inner) {
  const parts = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < inner.length; i++) {
    const c = inner[i];
    if (c === '"') i = literalEnd(inner, i) - 1;
    else if (c in OPEN) depth++;
    else if (CLOSE.has(c)) depth--;
    else if (c === "," && depth === 0) {
      parts.push(inner.slice(start, i));
      start = i + 1;
    }
  }
  parts.push(inner.slice(start));
  return parts.map((part) => part.trim()).filter(Boolean);
}

/**
 * Every `name!("key", arg = value, …)` call: its key, the names of its
 * named arguments, and its offset in `text`.
 */
export function macroCalls(text, name) {
  const calls = [];
  const pattern = new RegExp(`\\b${name}!\\(`, "g");
  for (const match of text.matchAll(pattern)) {
    const inner = balanced(text, match.index + match[0].length);
    if (inner === null) continue;
    const [first, ...rest] = topLevelArguments(inner);
    const key = /^"([^"]+)"$/.exec(first ?? "")?.[1];
    if (!key) continue;
    const names = rest.map((arg) => /^(\w+)\s*=(?!=)/.exec(arg)?.[1]).filter(Boolean);
    calls.push({ key, names, index: match.index });
  }
  return calls;
}
