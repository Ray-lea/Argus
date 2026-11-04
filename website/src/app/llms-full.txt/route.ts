import { markdownBody } from "@/lib/docs";
import { about, origin, stable } from "@/lib/site";
import { pathOf, source } from "@/lib/source";

export const dynamic = "force-static";

// Every page of the stable documentation in the sidebar's order, as one
// Markdown file for language models; llms.txt links each page on its own.
export async function GET() {
  const pages = new Map(
    source
      .getPages()
      .filter((page) => page.slugs[0] !== "dev")
      .map((page) => [page.data.source, page]),
  );
  const parts = [
    `# Argus documentation (${stable.label})\n\n> ${about.en.description}\n\nThe documentation is written in Chinese. 以下为 Argus 稳定版文档全文。`,
  ];
  for (const item of stable.nav.flatMap((group) => group.items)) {
    const page = pages.get(item.file);
    if (page) parts.push(`# ${page.data.heading}\n\n${origin + pathOf(page)}\n\n${(await markdownBody(page)).trim()}`);
  }
  return new Response(parts.join("\n\n---\n\n") + "\n", {
    headers: { "Content-Type": "text/plain; charset=utf-8" },
  });
}
