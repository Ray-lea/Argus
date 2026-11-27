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
  Calculator,
  ChartLine,
  ChartSpline,
  CircleHelp,
  Code,
  Command,
  Compass,
  Cpu,
  Download,
  Gauge,
  GitBranch,
  Globe,
  Hammer,
  History,
  KeyRound,
  Keyboard,
  Languages,
  Layers,
  LayoutDashboard,
  ListTree,
  Map,
  Monitor,
  MousePointerClick,
  Network,
  Palette,
  PanelTop,
  PanelsTopLeft,
  PenLine,
  Rocket,
  Scale,
  ServerCog,
  Settings,
  Shield,
  ShieldCheck,
  Tag,
  Terminal,
  TriangleAlert,
  Variable,
} from "lucide-react";
import { createElement } from "react";
import { z } from "zod";

// scripts/content.mjs writes both editions here before every build and development session.
const documentation = defineDocs({
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
    // Fumadocs' plugins plus two that keep GitHub's rendering of the same text:
    // - steps: headings numbered "1. ", "2. " at one level become a step list
    //   (GitHub shows numbered headings). Headings get their ids before this
    //   runs, so anchors match GitHub's.
    // - disclosures: <details><summary>…</summary> … </details> becomes an
    //   accordion, consecutive ones in one group (GitHub shows native
    //   disclosures). Raw HTML is otherwise dropped from these Markdown files.
    // The macro moves this function into a module of its own without this
    // file's imports, so it imports what it uses. A development server keeps
    // the result per collection name until it restarts: restart
    // start-website.bat after changing these options.
    mdxOptions: async (environment) => {
      const { applyMdxPreset } = await import("fumadocs-mdx/config");
      const { remarkSteps } = await import("fumadocs-core/mdx-plugins");
      type Node = { type: string; value?: string; name?: string; children?: Node[] };
      const opening = /^<details>\s*<summary>([\s\S]*?)<\/summary>$/;
      const jsx = (name: string, attributes: Record<string, string>, children: Node[]): Node =>
        Object.assign(
          { type: "mdxJsxFlowElement", name, children },
          {
            attributes: Object.entries(attributes).map(([key, value]) => ({ type: "mdxJsxAttribute", name: key, value })),
          },
        );
      const remarkDisclosures = () => (tree: Node) => {
        const html = (node: Node) => (node.type === "html" ? (node.value ?? "").trim() : "");
        const visit = (parent: Node) => {
          const nodes = parent.children;
          if (!nodes) return;
          const children: Node[] = [];
          for (let i = 0; i < nodes.length; i++) {
            const node = nodes[i];
            const summary = opening.exec(html(node));
            const end = summary ? nodes.findIndex((n, j) => j > i && html(n) === "</details>") : -1;
            if (!summary || end === -1) {
              visit(node);
              children.push(node);
              continue;
            }
            const body = nodes.slice(i + 1, end);
            body.forEach(visit);
            const item = jsx("Accordion", { title: summary[1].trim() }, body);
            const group = children.at(-1);
            if (group?.name === "Accordions" && group.children) group.children.push(item);
            else children.push(jsx("Accordions", { type: "multiple" }, [item]));
            i = end;
          }
          parent.children = children;
        };
        visit(tree);
      };
      return applyMdxPreset({ remarkPlugins: (plugins) => [...plugins, remarkSteps, remarkDisclosures] })(environment);
    },
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
  Calculator,
  ChartLine,
  ChartSpline,
  CircleHelp,
  Code,
  Command,
  Compass,
  Cpu,
  Download,
  Gauge,
  GitBranch,
  Globe,
  Hammer,
  History,
  KeyRound,
  Keyboard,
  Languages,
  Layers,
  LayoutDashboard,
  ListTree,
  Map,
  Monitor,
  MousePointerClick,
  Network,
  Palette,
  PanelTop,
  PanelsTopLeft,
  PenLine,
  Rocket,
  Scale,
  ServerCog,
  Settings,
  Shield,
  ShieldCheck,
  Tag,
  Terminal,
  TriangleAlert,
  Variable,
};

export const source = loader({
  baseUrl: "/docs",
  source: documentation.toFumadocsSource(),
  icon(name) {
    if (name && name in icons) return createElement(icons[name as keyof typeof icons]);
  },
});

export type DocsPage = NonNullable<ReturnType<typeof source.getPage>>;

// The exported folders end in a slash; Fumadocs' page.url does not.
export const pathOf = (page: { slugs: string[] }) => "/docs/" + page.slugs.map((slug) => slug + "/").join("");
