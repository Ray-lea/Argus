import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";
import { Provider } from "@/components/provider";
import { origin } from "./site";
import { choiceKeys, languageInfo, languages, type Language } from "./languages";
import { clientMessagesFor } from "./site-i18n";

export const metadata: Metadata = {
  metadataBase: new URL(origin),
  icons: { icon: { url: "/assets/argus.svg", type: "image/svg+xml" } },
  openGraph: { type: "website", siteName: "Argus" },
  twitter: { card: "summary_large_image" },
};
export const viewport: Viewport = {
  colorScheme: "light dark",
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: "#f3f4f7" },
    { media: "(prefers-color-scheme: dark)", color: "#0a0b0d" },
  ],
};

/*
 * Runs before the page is first drawn. Arriving from the other language (the
 * language switch stores where the reader was, LanguageSwitch.tsx), the page
 * holds its first frame until that section is parsed, opens at the same place
 * in it, and tags the view transition between the two pages as "lang"; the
 * leaving page tags it too.
 */
const place = "(()=>{const k=\"argus-place\",read=()=>{try{return JSON.parse(sessionStorage.getItem(k)||\"null\")}catch{return null}},pending=()=>{const p=read();return p&&p.to===location.pathname&&Date.now()-p.at<15000?p:null};let restored=false;const first=pending();if(first){document.documentElement.dataset.argusLanguageLanding=first.to;if(first.id){const l=document.createElement(\"link\");l.rel=\"expect\";l.href=\"#\"+first.id;l.setAttribute(\"blocking\",\"render\");document.head.append(l)}}const restore=transition=>{const p=pending();if(!p||restored)return;restored=true;const h=document.documentElement;h.dataset.argusLanguageLanding=p.to;transition?.types?.add(\"lang\");const pad=parseFloat(getComputedStyle(h).scrollPaddingTop)||0,el=p.id&&document.getElementById(p.id);scrollTo({top:el?el.getBoundingClientRect().top+scrollY-pad+p.within*el.offsetHeight:p.ratio*(h.scrollHeight-innerHeight),behavior:\"instant\"});};addEventListener(\"pagereveal\",e=>restore(e.viewTransition),{once:true});addEventListener(\"pageshow\",()=>restore(),{once:true});addEventListener(\"pageswap\",e=>{const p=read();if(e.viewTransition&&p&&e.activation&&new URL(e.activation.entry.url).pathname===p.to)e.viewTransition.types?.add(\"lang\")})})()";

/*
 * The home page's address opens the language a visitor picked (languages.ts)
 * when they arrive from elsewhere. Links within the site, reloading and going
 * back keep the page asked for; a visitor who never picked one, a search
 * engine among them, stays here. The page is hidden so that it does not
 * flash before the other one.
 */
const entry = `(()=>{try{if(location.pathname!=="/")return;const l=localStorage.getItem(${JSON.stringify(choiceKeys.chosen)});if(!${JSON.stringify(languages.map(item => item.id).filter(id => id !== "zh"))}.includes(l))return;const n=performance.getEntriesByType("navigation")[0];if(n&&n.type!=="navigate")return;if(document.referrer&&new URL(document.referrer).origin===location.origin)return;location.replace("/"+l+"/"+location.search+location.hash);document.documentElement.hidden=true}catch{}})()`;

// Each language has its own root layout; Chinese documentation shares the default one.
export function RootLayout({ lang, children }: { lang: Language; children: ReactNode }) {
  return (
    <html lang={languageInfo(lang).tag} suppressHydrationWarning>
      <head>
        {lang === "zh" && <script dangerouslySetInnerHTML={{ __html: entry }} />}
        <script dangerouslySetInnerHTML={{ __html: place }} />
      </head>
      <body>
        <Provider lang={lang} messages={clientMessagesFor(lang)}>{children}</Provider>
      </body>
    </html>
  );
}
