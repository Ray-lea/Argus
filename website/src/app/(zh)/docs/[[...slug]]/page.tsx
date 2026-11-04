import { Callout } from "fumadocs-ui/components/callout";
import { Card, Cards } from "fumadocs-ui/components/card";
import {
  DocsBody,
  DocsPage,
  EditOnGitHub,
  MarkdownCopyButton,
  ViewOptionsPopover,
} from "fumadocs-ui/layouts/spacious/page";
import defaultMdxComponents from "fumadocs-ui/mdx";
import { BookOpen, Code, Layers, Map, Palette, TriangleAlert } from "lucide-react";
import type { Metadata } from "next";
import { notFound } from "next/navigation";
import type { ComponentProps, FC } from "react";
import {
  articleSchema,
  breadcrumbSchema,
  canonicalPath,
  editionOf,
  editionSummary,
  markdownUrl,
  pageDescription,
  pageTitle,
  resolveHref,
} from "@/lib/docs";
import { previewImage } from "@/lib/page";
import { development, jsonLd, origin, repo, stable } from "@/lib/site";
import { pathOf, source } from "@/lib/source";

type Props = { params: Promise<{ slug?: string[] }> };

const date = (value: string) => {
  const [year, month, day] = value.split("-").map(Number);
  return year + " 年 " + month + " 月 " + day + " 日";
};

// The chapters of docs/README.md on the documentation home.
const chapters: Record<string, { icon: FC; text: string }> = {
  使用指南: { icon: BookOpen, text: "界面与操作、常驻与快捷键、设置项，以及硬件数据的来源与限制。" },
  开发指南: { icon: Code, text: "开发环境与命令、构建缓存、性能测量、提交发布与代码签名。" },
  架构: { icon: Layers, text: "采集层、宿主、特权服务与前端等模块的职责和边界。" },
  设计: { icon: Palette, text: "设计原则，以及曲线、窗口和全局快捷键的取舍。" },
};
const others: Record<string, FC> = { 路线图: Map, 已知问题: TriangleAlert };

export default async function Page({ params }: Props) {
  const page = source.getPage((await params).slug);
  if (!page) notFound();
  const edition = editionOf(page);
  const home = page.data.source === "docs/README.md";
  const { body: MDX, toc } = await page.data.load();
  const Link = defaultMdxComponents.a;
  return (
    <DocsPage toc={toc}>
      <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: jsonLd(breadcrumbSchema(page)) }} />
      {!home && (
        <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: jsonLd(articleSchema(page)) }} />
      )}
      {/* Earlier links to the page used GitHub's fragment for the title. */}
      <span id={page.data.anchor} />
      <h1 className="text-[1.75em] font-semibold">{page.data.heading}</h1>
      {home && <p className="mb-2 text-lg text-fd-muted-foreground">{editionSummary(edition)}</p>}
      {!home && (
        <div className="mb-6 flex flex-row flex-wrap items-center gap-2 border-b pb-6">
          <MarkdownCopyButton markdownUrl={markdownUrl(page)} />
          <ViewOptionsPopover
            markdownUrl={markdownUrl(page)}
            githubUrl={repo + "/blob/" + edition.ref + "/" + page.data.source}
            pageUrl={origin + pathOf(page)}
          />
        </div>
      )}
      <DocsBody className="docs-prose flex-1 text-fd-foreground/90">
        {/* The home's introduction only names the chapters the cards show. */}
        {!home && (
          <MDX
            components={{
              ...defaultMdxComponents,
              a: (props: ComponentProps<"a">) => (
                <Link {...props} href={resolveHref(edition, page.data.source, props.href)} />
              ),
              blockquote: Callout as unknown as FC<ComponentProps<"blockquote">>,
            }}
          />
        )}
        {home && (
          <>
            <Cards>
              {edition.nav
                .filter((group) => group.label in chapters)
                .map((group) => {
                  const Icon = chapters[group.label].icon;
                  return (
                    <Card key={group.label} icon={<Icon />} title={group.label} href={group.items[0].path}>
                      {chapters[group.label].text}
                    </Card>
                  );
                })}
            </Cards>
            <Callout title={edition === development ? "正在阅读开发版文档" : "正在阅读" + edition.label + "文档"}>
              {edition === development ? (
                <>
                  开发版跟随 main 分支更新，包含尚未发布的改动。已安装的版本请参阅
                  <Link href={stable.base}>稳定版文档</Link>。
                </>
              ) : (
                <>
                  稳定版与当前正式版一同发布。尚未发布的改动见
                  <Link href={development.base}>开发版文档</Link>。
                </>
              )}
            </Callout>
            <Cards>
              {edition.nav
                .filter((group) => !(group.label in chapters))
                .flatMap((group) => group.items)
                .map((item) => {
                  const Icon = others[item.label] ?? Map;
                  return <Card key={item.path} icon={<Icon />} title={item.label} href={item.path} />;
                })}
            </Cards>
          </>
        )}
      </DocsBody>
      <div className="flex flex-row flex-wrap items-center justify-between gap-4 empty:hidden">
        {page.data.updated && (
          <p className="text-sm text-fd-muted-foreground">最后更新于 {date(page.data.updated)}</p>
        )}
        {edition === development && <EditOnGitHub href={repo + "/edit/main/" + page.data.source} />}
      </div>
    </DocsPage>
  );
}

export function generateStaticParams() {
  return source.generateParams();
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const page = source.getPage((await params).slug);
  if (!page) notFound();
  const title = pageTitle(page),
    description = pageDescription(page),
    home = page.data.source === "docs/README.md";
  return {
    title: { absolute: title },
    description,
    alternates: {
      canonical: origin + canonicalPath(page),
      // The same page as Markdown, for language models and other tools.
      types: home ? undefined : { "text/markdown": origin + markdownUrl(page) },
    },
    // A page's openGraph replaces the layout's, so the shared fields repeat here.
    openGraph: {
      type: home ? "website" : "article",
      siteName: "Argus",
      title,
      description,
      url: origin + pathOf(page),
      images: previewImage("zh"),
      locale: "zh_CN",
      ...(!home && page.data.updated && { modifiedTime: page.data.updated }),
    },
  };
}
