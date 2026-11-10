export type Messages = Record<string, string>;
const patterns = new WeakMap<Messages, { match: RegExp; names: string[]; text: string }[]>();

export function translate(messages: Messages, source: string): string {
  if (Object.hasOwn(messages, source)) return messages[source];
  const core = source.trim();
  if (core !== source && Object.hasOwn(messages, core)) return source.replace(core, messages[core]);
  let rules = patterns.get(messages);
  if (!rules) {
    rules = Object.entries(messages).filter(([key]) => /\{\w+\}/.test(key)).map(([key, text]) => {
      const names: string[] = [];
      const expression = key.split(/(\{\w+\})/).map(part => {
        const hole = /^\{(\w+)\}$/.exec(part);
        if (hole) { names.push(hole[1]); return "(.+?)"; }
        return part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      }).join("");
      return { match: new RegExp("^" + expression + "$", "s"), names, text };
    });
    patterns.set(messages, rules);
  }
  for (const rule of rules) {
    const match = rule.match.exec(source);
    if (match) return rule.text.replace(/\{(\w+)\}/g, (hole, name: string) => {
      const index = rule.names.indexOf(name);
      return index < 0 ? hole : match[index + 1];
    });
  }
  return source;
}

// Copy objects also contain arrays and small formatters; identifiers stay literal.
export function translateValue<T>(messages: Messages, value: T): T {
  if (typeof value === "string") return translate(messages, value) as T;
  if (typeof value === "function") return ((...args: unknown[]) => translateValue(messages, value(...args))) as T;
  if (Array.isArray(value)) return value.map(item => translateValue(messages, item)) as T;
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, translateValue(messages, item)])) as T;
  return value;
}
