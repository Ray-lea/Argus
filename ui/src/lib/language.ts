// The language setting: stored natively (the tray and error messages follow
// it too), applied to the UI from what the host resolves.

import { type Language, setLocale } from "./i18n";
import { api } from "./ipc";
import type { SettingsView } from "./types";

export async function changeLanguage(language: Language): Promise<SettingsView> {
  const view = await api.setLanguage(language);
  setLocale(view.locale);
  return view;
}
