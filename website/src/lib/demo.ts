import catalogs from "../../content/replica.json";
import type { Language } from "./languages";
import type { Dictionary } from "@/components/replica/i18n";
export const demoDictionary = (lang: Language): Dictionary => catalogs[lang] as Dictionary;
