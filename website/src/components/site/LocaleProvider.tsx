"use client";
import { createContext, useContext, useMemo, type ReactNode } from "react";
import type { Language } from "@/lib/languages";
import { translateValue, type Messages } from "@/lib/translate";

const LocaleContext = createContext<{ lang: Language; messages: Messages }>({ lang: "zh", messages: {} });
export function LocaleProvider({ lang, messages, children }: { lang: Language; messages: Messages; children: ReactNode }) {
  return <LocaleContext value={{ lang, messages }}>{children}</LocaleContext>;
}
export function useSiteCopy<T>(base: { zh: T; en: T }): T {
  const { lang, messages } = useContext(LocaleContext);
  return useMemo(() => lang === "zh" ? base.zh : translateValue(messages, base.en), [lang, messages, base]);
}
export function useSiteText() {
  const { messages } = useContext(LocaleContext);
  return <T,>(value: T): T => translateValue(messages, value);
}
