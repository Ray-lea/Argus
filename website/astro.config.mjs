import { defineConfig } from "astro/config";
import { unified } from "@astrojs/markdown-remark";
import react from "@astrojs/react";
import starlight from "@astrojs/starlight";
import sitemap from "@astrojs/sitemap";
import tailwind from "@tailwindcss/vite";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { siteInput, workDir } from "./scripts/paths.mjs";
import releaseLinks from "./scripts/remark-links.mjs";

const groups = [
  ["guide", "使用指南"],
  ["architecture", "架构"],
  ["development", "开发指南"],
  ["design", "设计"],
];
const { editions } = JSON.parse(
  readFileSync(join(siteInput(), "site.json"), "utf8"),
);
function editionSidebar(edition) {
  const entries = Object.entries(edition.routes);
  const item = ([, path]) => ({ slug: path.slice(1, -1) });
  const chapters = groups
    .map(([directory, label]) => ({
      label,
      collapsed: true,
      items: entries
        .filter(([file]) => file.startsWith("docs/" + directory + "/"))
        .sort(
          ([a], [b]) =>
            Number(b.endsWith("/README.md")) - Number(a.endsWith("/README.md")),
        )
        .map(item),
    }))
    .filter((group) => group.items.length);
  const extra = entries.filter(
    ([file]) =>
      file !== "docs/README.md" &&
      !groups.some(([dir]) => file.startsWith("docs/" + dir + "/")),
  );
  return {
    label: edition.label,
    collapsed: edition.base !== "/docs/",
    items: [
      { label: "文档首页", slug: edition.base.slice(1, -1) },
      ...chapters,
      ...extra.map(item),
    ],
  };
}
export default defineConfig({
  site: "https://argustask.com",
  output: "static",
  outDir: join(workDir, "dist/site"),
  publicDir: join(siteInput(), "public"),
  trailingSlash: "always",
  build: { inlineStylesheets: "never" },
  security: {
    csp: {
      directives: [
        "default-src 'none'",
        "img-src 'self' data: https://raw.githubusercontent.com https://github.com",
        "font-src 'self' data:",
        "connect-src 'self'",
        "worker-src 'self' blob:",
        "form-action 'self'",
        "base-uri 'none'",
      ],
      // Starlight's early theme, search shortcut and sidebar scripts are emitted as is:inline.
      // verify.mjs checks the bytes of every executable inline script after each build.
      scriptDirective: {
        resources: ["'self'", "'wasm-unsafe-eval'"],
        hashes: [
          "sha256-VWo5Wp4aqSj6nSgMpeAp9cKieaoIfwFUAunAVugI5gA=",
          "sha256-f/zAUE74ucc3JYp4r4QQvkJofoQdkOIhHYK+jeZ6eko=",
          "sha256-GkZBRnvSuhtx/cvzvukVkX2JJZW+DdPlVr7BX8Tefqo=",
          "sha256-wX2yOADeV+NMngflD5uYi3vl50SHC4sfM1EmylVjlX4=",
          "sha256-7eCV4jtsr4t4knb3c4FCRPeu7GGZeOUGE3XvWix0XOQ=",
        ],
      },
      styleDirective: {
        resources: [
          { resource: "'self'", kind: "element" },
          { resource: "'unsafe-inline'", kind: "attribute" },
        ],
      },
    },
  },
  markdown: {
    syntaxHighlight: false,
    processor: unified({ remarkPlugins: [releaseLinks] }),
  },
  integrations: [
    react(),
    sitemap({
      filter: (url) =>
        !["/404.html", "/404/", "/search/"].includes(new URL(url).pathname),
    }),
    starlight({
      title: "Argus 文档",
      defaultLocale: "root",
      locales: { root: { label: "简体中文", lang: "zh-CN" } },
      favicon: "/assets/argus.svg",
      credits: false,
      disable404Route: true,
      expressiveCode: false,
      customCss: ["./src/styles/docs.css"],
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/Ray-lea/Argus",
        },
      ],
      components: {
        PageTitle: "./src/components/docs/PageTitle.astro",
        Footer: "./src/components/docs/Footer.astro",
        SiteTitle: "./src/components/docs/SiteTitle.astro",
      },
      sidebar: editions.map(editionSidebar),
    }),
  ],
  vite: {
    plugins: [tailwind()],
    build: { license: { fileName: "assets/bundle-licenses.md" } },
  },
});
