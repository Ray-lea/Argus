import { development, localized, repo, site, stable, text, type Language } from "@/lib/site";
import { SiteLink } from "./SiteLink";

export function Footer({ lang = "zh" }: { lang?: Language }) {
  const t = text[lang],
    en = lang === "en";
  const columns = [
    {
      title: en ? "Product" : "产品",
      links: [
        [localized(lang, "/download/"), t.download],
        [localized(lang, "/#faq"), t.questions],
        [localized(lang, "/changelog/"), t.changelog],
      ],
    },
    {
      title: en ? "Documentation (Chinese)" : "文档",
      links: [
        [stable.base, stable.label],
        [development.base, development.label],
        [stable.nav[0]?.items[0]?.path ?? stable.base, en ? "User guide" : "使用指南"],
      ],
    },
    {
      title: en ? "Project" : "项目",
      links: [
        [repo, "GitHub"],
        [repo + "/issues", en ? "Issues" : "问题反馈"],
        [repo + "/blob/main/LICENSE", "AGPL-3.0"],
      ],
    },
    {
      title: en ? "Policies" : "政策",
      links: [
        [localized(lang, "/privacy/"), t.privacy],
        [localized(lang, "/signing/"), t.signing],
        [localized(lang, "/security/"), t.security],
      ],
    },
  ];
  return (
    <footer className="site-wrap site-footer">
      <div className="footer-brand">
        <SiteLink lang={lang} className="brand" href={localized(lang)}>
          <img src="/assets/argus.svg" width="22" height="22" alt="" />
          Argus
        </SiteLink>
        <p>
          {en
            ? "A fast, modern, information-rich task manager for Windows."
            : "快速、现代、信息丰富的 Windows 任务管理器。"}
        </p>
      </div>
      <nav className="footer-links" aria-label={en ? "Site" : "站点"}>
        {columns.map((column) => (
          <div key={column.title}>
            <h2>{column.title}</h2>
            {column.links.map(([href, label]) => (
              <SiteLink lang={lang} key={label} href={href}>
                {label}
              </SiteLink>
            ))}
          </div>
        ))}
      </nav>
      <p className="footer-fine">
        © Argus contributors ·
        <a href={site.contact ? "mailto:contact@argustask.com" : repo + "/issues"}>{t.contact}</a>
      </p>
    </footer>
  );
}
