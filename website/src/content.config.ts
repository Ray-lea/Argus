import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";
import { docsSchema } from "@astrojs/starlight/schema";
import { join, sep } from "node:path";
import { pathToFileURL } from "node:url";
import { siteInput } from "../scripts/paths.mjs";

export const collections = {
  docs: defineCollection({
    loader: glob({
      base: pathToFileURL(join(siteInput(), "content") + sep),
      pattern: "**/*.md",
      generateId: ({ entry }) =>
        entry.replaceAll("\\", "/").replace(/\/index\.md$/, ""),
    }),
    schema: docsSchema(),
  }),
};
