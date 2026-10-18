// Regenerates src-tauri/installer-template.nsi from Tauri's NSIS template for
// the installed @tauri-apps/cli: the installer's and uninstaller's page lists
// are replaced by the pages in src-tauri/installer-ui.nsh, and deleting the
// app's data first ends the WebView2 processes of the Argus just closed and
// then says what is left; the rest is Tauri's. Run after upgrading Tauri; it
// stops if the template has changed where it is changed. See docs/development/release.md.
//
//   node scripts/nsis-template.mjs            fetch and write
//   node scripts/nsis-template.mjs <file>     use a downloaded template

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const out = join(root, "src-tauri", "installer-template.nsi");
const cli = JSON.parse(readFileSync(join(root, "node_modules/@tauri-apps/cli/package.json"), "utf8")).version;

async function upstream() {
  if (process.argv[2]) return readFileSync(process.argv[2], "utf8");
  const url = `https://raw.githubusercontent.com/tauri-apps/tauri/tauri-cli-v${cli}/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi`;
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return response.text();
}

/** Replaces the text from `start` up to `end` (exclusive); both must exist once. */
function splice(text, start, end, replacement) {
  const from = text.indexOf(start);
  const to = text.indexOf(end, from);
  if (from < 0 || to < 0 || text.indexOf(start, from + 1) >= 0) {
    throw new Error(`Tauri's template changed near "${start}": compare it with installer-template.nsi by hand`);
  }
  return text.slice(0, from) + replacement + text.slice(to);
}

const installerPages = `; Installer pages, must be ordered as they appear
; Argus: one page to install or upgrade, the progress page and a finish page,
; drawn by installer-ui.nsh, replace the welcome, license, install mode,
; reinstall, directory and finish pages. An existing installation is upgraded
; in place, as the updater does, never uninstalled first.
!define MUI_CUSTOMFUNCTION_GUIINIT ArgusGuiInit
AutoCloseWindow true

; 1. Install or upgrade (Argus)
Page custom ArgusPageMain ArgusPageMainLeave

; 2. Start menu shortcut page
Var AppStartMenuFolder
!if "\${STARTMENUFOLDER}" != ""
  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
  !define MUI_STARTMENUPAGE_DEFAULTFOLDER "\${STARTMENUFOLDER}"
!else
  !define MUI_PAGE_CUSTOMFUNCTION_PRE Skip
!endif
!insertmacro MUI_PAGE_STARTMENU Application $AppStartMenuFolder

; 3. Installation page (Argus: restyled)
!define MUI_PAGE_CUSTOMFUNCTION_SHOW ArgusPageProgress
!insertmacro MUI_PAGE_INSTFILES

; 4. Finish page (Argus)
Page custom ArgusPageFinish ArgusPageFinishLeave

Function RunMainBinary
  nsis_tauri_utils::RunAsUser "$INSTDIR\\\${MAINBINARYNAME}.exe" ""
FunctionEnd

`;

const uninstallerPages = `; Uninstaller Pages
; Argus: a confirmation page (with the "delete app data" choice), the progress
; page and a done page, drawn by installer-ui.nsh, replace the confirm page.
!define MUI_CUSTOMFUNCTION_UNGUIINIT un.ArgusGuiInit
Var DeleteAppDataCheckboxState

; 1. Confirm uninstall page (Argus)
UninstPage custom un.ArgusPageConfirm un.ArgusPageConfirmLeave

; 2. Uninstalling Page (Argus: restyled)
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.ArgusPageProgress
!insertmacro MUI_UNPAGE_INSTFILES

; 3. Done page (Argus)
UninstPage custom un.ArgusPageDone

`;

let text = (await upstream()).replace(/\r\n/g, "\n");
text = splice(text, "; Installer pages, must be ordered as they appear", "; Uninstaller Pages", installerPages);
text = splice(text, "; Uninstaller Pages", ";Languages", uninstallerPages);
const languages = '{{#each language_files}}\n  !include "{{this}}"\n{{/each}}\n';
text = splice(
  text,
  languages,
  "\nFunction .onInit",
  `${languages}; Argus: the pages; ARGUS_DIR is defined by installer-hooks.nsh.\n!include "\${ARGUS_DIR}\\installer-ui.nsh"\n`,
);

// Deleting the app's data: first the WebView2 processes of the Argus Restart
// Manager has just closed, which would keep their folder (leftovers.rs), then
// a word on whatever is left.
const insertAfter = (line, block) => {
  const uninstall = text.indexOf("\nSection Uninstall\n");
  const sectionEnd = text.indexOf("\nSectionEnd", uninstall);
  const at = text.indexOf(line, uninstall);
  const again = text.indexOf(line, at + 1);
  if (uninstall < 0 || at < 0 || at > sectionEnd || (again >= 0 && again < sectionEnd)) {
    throw new Error(`Tauri's template changed near "${line.trim()}": compare it with installer-template.nsi by hand`);
  }
  text = text.slice(0, at + line.length) + block + text.slice(at + line.length);
};
insertAfter(
  '  !insertmacro CheckIfAppIsRunning "$INSTDIR\\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"\n',
  `
  ; Argus: the WebView2 processes of the Argus just closed can outlive it for
  ; minutes and hold its data folder, which is deleted below when asked.
  \${If} $DeleteAppDataCheckboxState = 1
  \${AndIf} $UpdateMode <> 1
    ExecWait '"$INSTDIR\\\${MAINBINARYNAME}.exe" --argus-close-copies'
  \${EndIf}
`,
);
insertAfter(
  '    RmDir /r "$LOCALAPPDATA\\${BUNDLEID}"\n',
  `
    ; Argus: say what is left rather than appear to have deleted it.
    StrCpy $R0 ""
    \${If} \${FileExists} "$APPDATA\\\${BUNDLEID}\\*.*"
      StrCpy $R0 "$R0$\\r$\\n$APPDATA\\\${BUNDLEID}"
    \${EndIf}
    \${If} \${FileExists} "$LOCALAPPDATA\\\${BUNDLEID}\\*.*"
      StrCpy $R0 "$R0$\\r$\\n$LOCALAPPDATA\\\${BUNDLEID}"
    \${EndIf}
    \${If} $R0 != ""
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(ArgusDataLeft)$\\r$\\n$R0" /SD IDOK
    \${EndIf}
`,
);

const header = `; Argus's NSIS template: Tauri's template (tauri-cli ${cli}) with the installer's
; and uninstaller's pages replaced and deleting the app's data extended; every
; change is marked "Argus". Generated by
; scripts/nsis-template.mjs, which reapplies the changes after a Tauri upgrade.
`;
writeFileSync(out, header + text);
console.log(`wrote ${out} from tauri-cli ${cli}`);
