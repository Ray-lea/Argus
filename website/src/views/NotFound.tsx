import { SiteFrame } from "@/components/site/SiteFrame";
import { pageMetadata } from "@/lib/page";

const title = "页面未找到 · Argus";

export const notFoundMetadata = pageMetadata({
  lang: "zh",
  path: "/404.html",
  title,
  description: "前往 Argus 官网或文档。",
  alternate: false,
  noindex: true,
});

export function NotFound() {
  return (
    <SiteFrame lang="zh" path="/404.html" title={title} alternate={false} noindex>
      <div className="site-wrap">
        <header className="page-head">
          <h1>页面未找到</h1>
          <p>
            <a className="text-link" href="/">
              返回首页
            </a>
            ·
            <a className="text-link" href="/docs/">
              浏览文档
            </a>
          </p>
        </header>
      </div>
    </SiteFrame>
  );
}
