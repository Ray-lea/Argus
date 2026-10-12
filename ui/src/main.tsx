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

// Know the language before the first render, so no text shows in the wrong one.
const info = await api.appInfo().catch(() => null);
setLocale(info?.locale ?? localeFor(navigator.language));
setAppInfo(info);

render(() => <App />, document.getElementById("root") as HTMLElement);
