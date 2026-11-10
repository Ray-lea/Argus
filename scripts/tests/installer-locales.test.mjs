import assert from "node:assert/strict";
import test from "node:test";
import { resolve } from "node:path";
import { installerLanguages, installerStrings, nsisText } from "../lib/installer-locales.mjs";

test("installer text escapes literals and expands only known NSIS variables", () => {
  assert.equal(nsisText('"$TEMP"\n{version} {installDir} {code}'), '$\\"$$TEMP$\\"$\\r$\\n${DISPLAYVERSION} $INSTDIR $0');
  assert.throws(() => nsisText("{unknown}"), /Unknown installer placeholder/);
});
test("every installer language includes pages, hooks and file replacement errors", () => {
  const text = installerStrings(resolve(import.meta.dirname, "../.."));
  for (const language of Object.values(installerLanguages)) {
    for (const key of ["ArgusFace", "ArgusInstall", "ArgusRemoveService", "ArgusCopyFailed", "appRunning"]) {
      assert.ok(text.includes(`LangString ${key} \${LANG_${language}} `), `${language}: ${key}`);
    }
  }
  assert.ok(!text.includes("{installDir}"));
});
