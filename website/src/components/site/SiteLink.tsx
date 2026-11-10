import { IntentLink } from "./IntentLink";
import type { ComponentPropsWithoutRef } from "react";
import type { Language } from "@/lib/site";
import { languageOfPath } from "@/lib/languages";

// Pages in the same language navigate in place. A different language, a raw
// file or another site loads a new document. Documentation uses the default root.
const sameRoot = (lang: Language, href: string) => {
  if (!href.startsWith("/") || href.startsWith("//")) return false;
  const path = href.split(/[?#]/)[0];
  if (!path.endsWith("/")) return false;
  return languageOfPath(path) === lang;
};

type Props = ComponentPropsWithoutRef<"a"> & { lang: Language; href: string };

export function SiteLink({ lang, href, ...props }: Props) {
  return sameRoot(lang, href) ? <IntentLink href={href} inPage {...props} /> : <a href={href} {...props} />;
}
