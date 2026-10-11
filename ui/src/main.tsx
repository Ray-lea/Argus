import { render } from "solid-js/web";
import App from "./App";
import { inTauri } from "./lib/ipc";
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

render(() => <App />, document.getElementById("root") as HTMLElement);
