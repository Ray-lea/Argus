"use client";
import type { ReactNode } from "react";
import { registerDictionary, setLanguage, type Dictionary, type Language } from "./i18n";
// Register before the window frame, tour and drop area render their text.
export function DemoLanguage({ lang, dictionary, children }: { lang: Language; dictionary: Dictionary; children: ReactNode }) {
  setLanguage(lang);
  registerDictionary(dictionary);
  return children;
}
