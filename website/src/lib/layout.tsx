import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";
import { Provider } from "@/components/provider";
import { origin } from "./site";

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
const place = `(()=>{const k="argus-place",read=()=>{try{return JSON.parse(sessionStorage.getItem(k)||"null")}catch{return null}};const p=read(),fresh=p&&p.to===location.pathname&&Date.now()-p.at<15000;if(fresh&&p.id){const l=document.createElement("link");l.rel="expect";l.href="#"+p.id;l.setAttribute("blocking","render");document.head.append(l)}addEventListener("pagereveal",e=>{if(!fresh)return;try{sessionStorage.removeItem(k)}catch{}e.viewTransition?.types.add("lang");const h=document.documentElement,pad=parseFloat(getComputedStyle(h).scrollPaddingTop)||0,el=p.id&&document.getElementById(p.id);scrollTo({top:el?el.getBoundingClientRect().top+scrollY-pad+p.within*el.offsetHeight:p.ratio*(h.scrollHeight-innerHeight),behavior:"instant"})},{once:true});addEventListener("pageswap",e=>{const q=read();if(e.viewTransition&&q&&e.activation&&new URL(e.activation.entry.url).pathname===q.to)e.viewTransition.types.add("lang")})})()`;

// Chinese pages and the documentation share one root layout, English pages another.
export function RootLayout({ lang, children }: { lang: "zh" | "en"; children: ReactNode }) {
  return (
    <html lang={lang === "en" ? "en" : "zh-CN"} suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: place }} />
      </head>
      <body>
        <Provider lang={lang}>{children}</Provider>
      </body>
    </html>
  );
}
