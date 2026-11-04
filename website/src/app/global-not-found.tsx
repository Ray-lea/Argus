import type { Metadata } from "next";
import { metadata as root, RootLayout } from "@/lib/layout";
import { NotFound, notFoundMetadata } from "@/views/NotFound";
import "@/styles/app.css";

// Unmatched URLs bypass both root layouts, so this page brings its own.
export { viewport } from "@/lib/layout";
export const metadata: Metadata = { ...root, ...notFoundMetadata };

export default function GlobalNotFound() {
  return (
    <RootLayout lang="zh">
      <NotFound />
    </RootLayout>
  );
}
