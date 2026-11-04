import type { ReactNode } from "react";
import { pageGraph } from "@/lib/page";
import { jsonLd, text, type Language } from "@/lib/site";
import { Footer } from "./Footer";
import { Header } from "./Header";
import { MotionGate } from "./MotionGate";
import { Spotlight } from "./Spotlight";

interface Props {
  lang: Language;
  path: string;
  title: string;
  alternate?: boolean;
  noindex?: boolean;
  schema?: unknown;
  footer?: boolean;
  children: ReactNode;
}

// The frame of the site's own pages; head tags come from pageMetadata.
export function SiteFrame({
  lang,
  path,
  title,
  alternate = true,
  noindex = false,
  schema,
  footer = true,
  children,
}: Props) {
  const graph = pageGraph({ lang, path, title, noindex, schema });
  return (
    <>
      <a className="skip" href="#main">
        {text[lang].skip}
      </a>
      <Header lang={lang} path={path} alternate={alternate} />
      <main id="main">{children}</main>
      {footer && <Footer lang={lang} />}
      <Spotlight />
      <MotionGate />
      {graph.map((item, i) => (
        <script key={i} type="application/ld+json" dangerouslySetInnerHTML={{ __html: jsonLd(item) }} />
      ))}
    </>
  );
}
