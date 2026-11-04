import { SiteFrame } from "@/components/site/SiteFrame";
import { pageMetadata } from "@/lib/page";
import { repo, site, text, type Language, type Policy as PolicyName } from "@/lib/site";

const summary = (lang: Language, policy: PolicyName) => {
  const en = lang === "en";
  return {
    privacy: en
      ? "How Argus stores data and connects to update services."
      : "Argus 的本地数据存储、更新请求与网络连接。",
    signing: en ? "The project's public code signing policy." : "Argus 项目的公开代码签名策略。",
    security: en
      ? "Report a vulnerability privately to the Argus maintainers."
      : "向 Argus 维护者私下报告安全漏洞。",
  }[policy];
};

export const policyMetadata = (lang: Language, policy: PolicyName) =>
  pageMetadata({
    lang,
    path: "/" + policy + "/",
    title: text[lang][policy] + " · Argus",
    description: summary(lang, policy),
  });

export function Policy({ lang, policy }: { lang: Language; policy: PolicyName }) {
  const en = lang === "en",
    title = text[lang][policy];
  return (
    <SiteFrame lang={lang} path={"/" + policy + "/"} title={title + " · Argus"}>
      <div className="site-wrap">
        <header className="page-head">
          <h1 data-rise="">{title}</h1>
          <p data-rise="2">{summary(lang, policy)}</p>
        </header>
        <article className="panel" data-rise="3">
          {policy === "security" ? (
            <div className="site-prose">
              <p>
                {en
                  ? "Send the affected Argus and Windows versions, reproduction steps and expected impact to the maintainers."
                  : "请提供受影响的 Argus 与 Windows 版本、复现步骤和可能的影响。"}
              </p>
              <ul>
                <li>
                  <a href={repo + "/security/advisories/new"}>
                    {en ? "GitHub private vulnerability report" : "GitHub 私密漏洞报告"}
                  </a>
                </li>
                {site.contact && (
                  <li>
                    <a href="mailto:security@argustask.com">security@argustask.com</a>
                  </li>
                )}
              </ul>
            </div>
          ) : (
            <div
              className="site-prose"
              dangerouslySetInnerHTML={{ __html: site.translations[lang][policy] }}
            />
          )}
        </article>
      </div>
    </SiteFrame>
  );
}
