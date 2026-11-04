import { ViewTransition, type ReactNode } from "react";
import { pageGraph } from "@/lib/page";
import { jsonLd, text, type Language } from "@/lib/site";
import { Disclosure } from "./Disclosure";
import { Footer } from "./Footer";
import { Header } from "./Header";
import { HashLanding } from "./IntentLink";
import { MotionGate } from "./MotionGate";
import { Spotlight } from "./Spotlight";

interface Props {
  lang: Language;
  path: string;
  title: string;
  alternate?: boolean;
  noindex?: boolean;
  schema?: object | object[];
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
      {/* Moving to another page, this one leaves and the next rises into place
          (global.css); the header stays where it is. */}
      <ViewTransition enter="page-enter" exit="page-exit" default="none">
        <div className="page">
          <main id="main">{children}</main>
          {footer && <Footer lang={lang} />}
        </div>
      </ViewTransition>
      <Spotlight />
      <Disclosure />
      <HashLanding />
      <MotionGate />
      {graph.map((item, i) => (
        <script key={i} type="application/ld+json" dangerouslySetInnerHTML={{ __html: jsonLd(item) }} />
      ))}
    </>
  );
}
