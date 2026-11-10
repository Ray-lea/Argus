import { copyFor, localize } from "@/lib/site-i18n";
import { development, localized, repo, site, stable, text, type Language } from "@/lib/site";
import { SiteLink } from "./SiteLink";

export function Footer({ lang = "zh" }: { lang?: Language }) {
  const t = text[lang],
    en = lang !== "zh";
  const columns = localize(lang, [
    {
      title: localize(lang, en ? "Product" : "产品"),
      links: [
        [localized(lang, "/download/"), t.download],
        [localized(lang, "/#faq"), t.questions],
        [localized(lang, "/changelog/"), t.changelog],
      ],
    },
    {
      title: localize(lang, en ? "Documentation (Chinese)" : "文档"),
      links: [
        [stable.base, localize(lang, en ? "Stable" : "稳定版") + " " + site.release.tag],
        [development.base, localize(lang, en ? "Development" : "开发版")],
        [stable.nav[0]?.items[0]?.path ?? stable.base, localize(lang, en ? "User guide" : "使用指南")],
      ],
    },
    {
      title: localize(lang, en ? "Project" : "项目"),
      links: [
        [repo, "GitHub"],
        [repo + "/issues", localize(lang, en ? "Issues" : "问题反馈")],
        [repo + "/blob/main/LICENSE", "AGPL-3.0"],
      ],
    },
    {
      title: localize(lang, en ? "Policies" : "政策"),
      links: [
        [localized(lang, "/privacy/"), t.privacy],
        [localized(lang, "/signing/"), t.signing],
        [localized(lang, "/security/"), t.security],
      ],
    },
  ]);
  return (
    <footer className="site-wrap site-footer">
      <div className="footer-brand">
        <SiteLink lang={lang} className="brand" href={localized(lang)}>
          <img src="/assets/argus.svg" width="22" height="22" alt="" />
          Argus
        </SiteLink>
        <p>
          {localize(lang, en
            ? "A fast, modern, information-rich task manager for Windows."
            : "快速、现代、信息丰富的 Windows 任务管理器。")}
        </p>
      </div>
      <nav className="footer-links" aria-label={localize(lang, en ? "Site" : "站点")}>
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
