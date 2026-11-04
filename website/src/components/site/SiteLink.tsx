import { IntentLink } from "./IntentLink";
import type { ComponentPropsWithoutRef } from "react";
import type { Language } from "@/lib/site";

// Chinese pages and the documentation share one root layout, English pages
// another. Pages under the same root navigate in place; anything else (the
// other language, raw files, other sites) loads a new document.
const sameRoot = (lang: Language, href: string) => {
  if (!href.startsWith("/") || href.startsWith("//")) return false;
  const path = href.split(/[?#]/)[0];
  if (!path.endsWith("/")) return false;
  return (lang === "en") === path.startsWith("/en/");
};

type Props = ComponentPropsWithoutRef<"a"> & { lang: Language; href: string };

export function SiteLink({ lang, href, ...props }: Props) {
  return sameRoot(lang, href) ? <IntentLink href={href} {...props} /> : <a href={href} {...props} />;
}
