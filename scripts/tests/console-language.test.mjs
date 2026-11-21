import assert from "node:assert/strict";
import { test } from "node:test";
import { consoleLocale, displayLanguage } from "../lib/console-language.mjs";

const output = (lines) => ["", "HKEY_CURRENT_USER\\Control Panel\\Desktop", ...lines, "", "End of search: 2 match(es) found."].join("\r\n");

test("the user's display language overrides the machine's", () => {
  const user = "    PreferredUILanguages    REG_MULTI_SZ    en-US";
  const previous = "    PreviousPreferredUILanguages    REG_MULTI_SZ    ja-JP";
  const machine = "    MachinePreferredUILanguages    REG_MULTI_SZ    zh-CN\\0en-US";
  assert.equal(displayLanguage(output([machine, previous, user])), "en-US");
  assert.equal(displayLanguage(output([previous, machine])), "zh-CN");
  assert.equal(displayLanguage(output([])), undefined);
});

test("ARGUS_LOCALE comes first, as in the application", () => {
  assert.equal(consoleLocale({ ARGUS_LOCALE: "zh-CN" }, () => "en-US"), "zh-CN");
  assert.equal(consoleLocale({}, () => "ru-RU"), "ru-RU");
  assert.ok(consoleLocale({}, () => undefined));
});
