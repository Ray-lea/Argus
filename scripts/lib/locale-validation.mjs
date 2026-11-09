// Catalog invariants independent of any translation's wording.
const TAG = /<(\/?)([A-Za-z]\w*)>/g;
export function markupError(text) {
  const stack = [];
  for (const [, closing, tag] of text.matchAll(TAG)) {
    if (!closing) stack.push(tag);
    else if (stack.pop() !== tag) return `unexpected closing tag </${tag}>`;
  }
  return stack.length ? `unclosed tag <${stack.at(-1)}>` : null;
}
export function textErrors(source, text) {
  const errors = [];
  if (source.trim() && !text.trim()) errors.push("empty translation");
  if (/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u200b\u202a-\u202e\u2066-\u2069\ufeff]/u.test(text)) errors.push("invisible control character");
  if (/^[QXZ]{1,3}$/m.test(text) && !/^[QXZ]{1,3}$/m.test(source)) errors.push("translation marker on its own line");
  const markup = markupError(text);
  if (markup) errors.push(markup);
  for (const identifier of technicalLiterals(source)) {
    if (!text.includes(identifier)) errors.push(`technical identifier changed or missing: ${identifier}`);
  }
  return errors;
}

export function technicalLiterals(source) {
  const fixed = ["before.debugger", "Program Files\\Argus Sensor Setup",
    "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Image File Execution Options\\taskmgr.exe"];
  return new Set([
    ...fixed.filter(value => source.includes(value)),
    ...[...source.matchAll(/\b(?:Debugger|UseFilter|null)\b/g)].map(m => m[0]),
    ...[...source.matchAll(/\b[\w.-]+\.(?:exe|dll|sys|json|log)\b/gi)].map(m => m[0]),
    ...[...source.matchAll(/--[a-z][a-z-]*\b/g)].map(m => m[0]),
  ]);
}
export function maySpellOne(locale) {
  // French includes zero; Russian includes 21, 31, etc. in category "one".
  return ["en-US", "de-DE", "es-ES"].includes(locale);
}
