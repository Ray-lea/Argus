import { render } from "solid-js/web";
import App from "./App";
import { localeFor, setLocale } from "./lib/i18n";
import { api, inTauri } from "./lib/ipc";
import { setAppInfo } from "./lib/store";
import "./styles.css";
import "./pages.css";
import "./tables.css";

document.documentElement.classList.toggle("tauri", inTauri);

// The webview's own context menu (reload, inspect) has no place in the app;
// keep it only where text can be edited or selected.
document.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement;
  if (!t.closest("input, textarea, .selectable")) e.preventDefault();
});

// Errors nothing else caught go to the log, which bug reports carry. Once the
// dev server has hot-swapped a module, a file saved halfway through an edit
// can throw (a name used one save before its import); that is no fault of
// Argus, so from then on the page's errors stay in the console only.
// Production builds have no `import.meta.hot` and report every error.
let hotSwapped = false;
import.meta.hot?.on("vite:beforeUpdate", () => {
  hotSwapped = true;
});
window.addEventListener("error", (e) => {
  if (!hotSwapped) api.logError("uncaught", e.error);
});
window.addEventListener("unhandledrejection", (e) => {
  if (!hotSwapped) api.logError("unhandledRejection", e.reason);
});

// Know the language before the first render, so no text shows in the wrong one.
const info = await api.appInfo().catch(() => null);
document.documentElement.classList.toggle("mica", inTauri && info?.mica === true);
setLocale(info?.locale ?? localeFor(navigator.language));
setAppInfo(info);

render(() => <App />, document.getElementById("root") as HTMLElement);
