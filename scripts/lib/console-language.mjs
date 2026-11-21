// Language of the development scripts' console messages: Simplified Chinese when
// Windows displays Chinese, English otherwise. Chosen as the application chooses its
// language (argus_core::i18n): ARGUS_LOCALE, then the user's display language, then
// the machine's. start.bat and start-website.bat repeat the check. Files the
// scripts generate keep their own languages.
import { spawnSync } from "node:child_process";

// The user's display language overrides the machine default (`reg query` output).
export function displayLanguage(output) {
  let user, machine;
  for (const line of output.split(/\r?\n/)) {
    const match = /^\s+(\S+)\s+REG_MULTI_SZ\s+(\S+)/.exec(line);
    const first = match?.[2].split("\\0")[0];
    if (match?.[1] === "PreferredUILanguages") user = first;
    else if (match?.[1] === "MachinePreferredUILanguages") machine = first;
  }
  return user ?? machine;
}

function windowsLanguage() {
  if (process.platform !== "win32") return undefined;
  const result = spawnSync("reg", ["query", "HKCU\\Control Panel\\Desktop", "/s", "/f", "PreferredUILanguages", "/v"], {
    encoding: "utf8",
    windowsHide: true,
    timeout: 3000,
  });
  return result.status === 0 ? displayLanguage(result.stdout) : undefined;
}

export function consoleLocale(environment = process.env, system = windowsLanguage) {
  return environment.ARGUS_LOCALE || system() || Intl.DateTimeFormat().resolvedOptions().locale;
}

export const chinese = /^zh(?:-|$)/i.test(consoleLocale());

/** The Chinese or the English message, by the console language. */
export const t = (zh, en) => (chinese ? zh : en);
