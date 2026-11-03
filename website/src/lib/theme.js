// This classic script runs in <head> before either layout paints.
(() => {
  const key = "starlight-theme";
  const system = window.matchMedia("(prefers-color-scheme: dark)");
  const normalize = (value) =>
    value === "dark" || value === "light" ? value : "auto";
  let preference = "auto";

  function read() {
    try {
      return normalize(localStorage.getItem(key));
    } catch {
      return preference;
    }
  }

  function apply(mode) {
    preference = mode;
    document.documentElement.dataset.theme =
      mode === "auto" ? (system.matches ? "dark" : "light") : mode;
    for (const picker of document.querySelectorAll("select[data-site-theme]"))
      picker.value = mode;
  }

  apply(read());
  document.addEventListener("DOMContentLoaded", () => apply(preference), {
    once: true,
  });
  document.addEventListener("change", (event) => {
    const picker = event.target;
    if (
      !(picker instanceof HTMLSelectElement) ||
      !picker.hasAttribute("data-site-theme")
    )
      return;
    apply(normalize(picker.value));
    try {
      if (preference === "auto") localStorage.removeItem(key);
      else localStorage.setItem(key, preference);
    } catch {
      // In restricted storage contexts the selection still applies to the current page.
    }
  });
  system.addEventListener("change", () => {
    if (preference === "auto") apply("auto");
  });
  window.addEventListener("storage", (event) => {
    if (event.key === key || event.key === null) apply(read());
  });
  window.addEventListener("pageshow", () => apply(read()));
})();
