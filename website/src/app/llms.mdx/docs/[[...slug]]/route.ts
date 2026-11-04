import { notFound } from "next/navigation";
import { source } from "@/lib/source";

export const revalidate = false;

// The Markdown of each document, for "Copy Markdown" and "View as Markdown".
export async function GET(_request: Request, { params }: { params: Promise<{ slug?: string[] }> }) {
  // The last segment is the appended "content.md".
  const page = source.getPage((await params).slug?.slice(0, -1));
  if (!page) notFound();
  return new Response("# " + page.data.heading + "\n\n" + (await page.data.getText("processed")), {
    headers: { "Content-Type": "text/markdown; charset=utf-8" },
  });
}

export function generateStaticParams() {
  return source.getPages().map((page) => ({ slug: [...page.slugs, "content.md"] }));
}
