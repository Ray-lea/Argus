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

// Chinese pages and the documentation share one root layout, English pages another.
export function RootLayout({ lang, children }: { lang: "zh" | "en"; children: ReactNode }) {
  return (
    <html lang={lang === "en" ? "en" : "zh-CN"} suppressHydrationWarning>
      <body>
        <Provider lang={lang}>{children}</Provider>
      </body>
    </html>
  );
}
