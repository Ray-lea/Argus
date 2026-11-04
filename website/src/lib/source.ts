import { loader } from "fumadocs-core/source";
import { metaSchema, pageSchema } from "fumadocs-core/source/schema";
import { defineDocs } from "fumadocs-mdx/macro";
import {
  Activity,
  AppWindow,
  ArrowLeftRight,
  BadgeCheck,
  BookMarked,
  BookOpen,
  ChartSpline,
  Command,
  Cpu,
  Gauge,
  GitBranch,
  Globe,
  Hammer,
  KeyRound,
  Keyboard,
  Languages,
  Layers,
  LayoutDashboard,
  Map,
  Monitor,
  MousePointerClick,
  Palette,
  PanelTop,
  PanelsTopLeft,
  Rocket,
  Settings,
  Shield,
  ShieldCheck,
  Terminal,
  TriangleAlert,
} from "lucide-react";
import { createElement } from "react";
import { z } from "zod";

// scripts/content.mjs writes both editions here before every build and development session.
const docs = defineDocs({
  dir: "content/docs",
  docs: {
    schema: pageSchema.extend({
      // The page's own title; `title` is the chapter name from the documentation index.
      heading: z.string(),
      source: z.string(),
      updated: z.string(),
      anchor: z.string(),
    }),
    // Each document compiles when it is first requested, so the development
    // server does not compile every page of both editions up front.
    async: true,
    // Plain Markdown for the "Copy Markdown" button and the .md view.
    postprocess: { includeProcessedMarkdown: true },
  },
  meta: { schema: metaSchema },
});

// The icons named in content/docs; listed one by one so the bundle holds only these.
const icons = {
  Activity,
  AppWindow,
  ArrowLeftRight,
  BadgeCheck,
  BookMarked,
  BookOpen,
  ChartSpline,
  Command,
  Cpu,
  Gauge,
  GitBranch,
  Globe,
  Hammer,
  KeyRound,
  Keyboard,
  Languages,
  Layers,
  LayoutDashboard,
  Map,
  Monitor,
  MousePointerClick,
  Palette,
  PanelTop,
  PanelsTopLeft,
  Rocket,
  Settings,
  Shield,
  ShieldCheck,
  Terminal,
  TriangleAlert,
};

export const source = loader({
  baseUrl: "/docs",
  source: docs.toFumadocsSource(),
  icon(name) {
    if (name && name in icons) return createElement(icons[name as keyof typeof icons]);
  },
});

export type DocsPage = NonNullable<ReturnType<typeof source.getPage>>;

// The exported folders end in a slash; Fumadocs' page.url does not.
export const pathOf = (page: { slugs: string[] }) => "/docs/" + page.slugs.map((slug) => slug + "/").join("");
