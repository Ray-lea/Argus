// Shared terminology policy for application and website catalogs.
import { readFileSync } from "node:fs";

export const terminology = JSON.parse(readFileSync(new URL("../../resources/terminology.json", import.meta.url), "utf8"));
const escape = value => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
// ASCII boundaries allow labels next to CJK text and hyphenated compounds,
// while keeping CPU distinct from CPUID and GB distinct from Gbps.
const token = value => new RegExp(`(?<![A-Za-z0-9_])${escape(value)}(?![A-Za-z0-9_])`, "u");
const plain = value => value.replace(/\{\w+\}|<\/?\w+>/g, " ");
const abbreviations = Object.entries(terminology.abbreviations).map(([name, aliases]) => ({
  name,
  pattern: token(name),
  aliases: aliases.map(alias => ({ alias, pattern: new RegExp(`(?<![\\p{L}\\p{N}_])${escape(alias)}(?![\\p{L}\\p{N}_])`, "u") })),
}));
const identifiers = terminology.identifiers.map(name => ({ name, pattern: token(name) }));
// Single-letter units need a quantity or a unit-only label; an English "A"
// at the start of a sentence is an article. Keep placeholders as quantities.
const units = terminology.units.map(name => ({
  name,
  pattern: new RegExp(`(?<![A-Za-z_])${escape(name)}(?![A-Za-z0-9_])`, "u"),
  quantity: new RegExp(`(?:\\d|\\{\\w+\\})\\s*(?:<\\w+>)?${escape(name)}(?![A-Za-z0-9_])`, "u"),
}));

export function terminologyErrors(source, text, { key = "" } = {}) {
  const errors = [];
  const original = plain(source), translated = plain(text);
  const exact = terminology.exactLabels[key];
  if (exact && text !== exact) errors.push(`label must be exactly ${exact}`);
  for (const { name, pattern, aliases } of abbreviations) {
    for (const { alias, pattern: aliasPattern } of aliases) {
      if (aliasPattern.test(translated)) errors.push(`use ${name} instead of ${alias}`);
    }
    if (!terminology.localizedPhrases[key]?.includes(name) && pattern.test(original) && !pattern.test(translated)) {
      errors.push(`abbreviation changed or missing: ${name}`);
    }
  }
  for (const { name, pattern } of identifiers) {
    if (pattern.test(original) && !pattern.test(translated)) errors.push(`technical term changed or missing: ${name}`);
  }
  for (const { name, pattern, quantity } of units) {
    const inSource = name.length === 1 ? source.trim() === name || quantity.test(source) : pattern.test(original);
    const inText = name.length === 1 ? text.trim() === name || quantity.test(text) : pattern.test(translated);
    if (inSource && !inText) errors.push(`unit changed or missing: ${name}`);
  }
  return errors;
}
