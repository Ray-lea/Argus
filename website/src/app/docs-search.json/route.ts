import { createFromSource } from "fumadocs-core/search/server";
import { pathOf, source } from "@/lib/source";

export const revalidate = false;

// Exported once at build time; the dialog searches it in the browser.
export const { staticGET: GET } = createFromSource(source, {
  language: "multilingual",
  buildIndex: async (page) => ({
    id: pathOf(page),
    url: pathOf(page),
    title: page.data.heading,
    description: page.data.description,
    structuredData: await page.data.structuredData(),
    tag: page.slugs[0] === "dev" ? "dev" : "stable",
  }),
});
