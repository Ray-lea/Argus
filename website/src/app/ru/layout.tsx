import type { ReactNode } from "react";
import { RootLayout } from "@/lib/layout";
import "@/styles/app.css";

export { metadata, viewport } from "@/lib/layout";

export default function Layout({ children }: { children: ReactNode }) {
  return <RootLayout lang="ru">{children}</RootLayout>;
}
