import { localized, repo, site, text, type Language } from "@/lib/site";
import { compact } from "@/components/home/OpenSource";
import { MastMenu, SearchButton, ThemePicker } from "./HeaderTools";
import { Icon } from "./Icon";
import { SiteLink } from "./SiteLink";

interface Props {
  lang: Language;
  path: string;
  alternate?: boolean;
  wide?: boolean;
}

export function Header({ lang, path, alternate = true, wide = false }: Props) {
  const t = text[lang],
    en = lang === "en";
  const links = [
    { href: "/docs/", label: t.docs, current: path.startsWith("/docs/") },
    { href: localized(lang, "/download/"), label: t.download, current: path === "/download/" },
    { href: localized(lang, "/changelog/"), label: t.changelog, current: path === "/changelog/" },
    { href: localized(lang, "/#faq"), label: t.questions, current: false },
  ];
  const stars = site.repository?.stars;
  const other = localized(en ? "zh" : "en", path);
  const menu = en ? "Menu" : "菜单";
  return (
    <header className={wide ? "mast is-wide" : "mast"}>
      <div className="mast-inner">
        <SiteLink lang={lang} className="brand" href={localized(lang)}>
          <img src="/assets/argus.svg" width="22" height="22" alt="" />
          Argus
        </SiteLink>
        <nav className="mast-nav" aria-label={en ? "Main" : "主导航"}>
          {links.map((link) => (
            <SiteLink
              lang={lang}
              key={link.href}
              href={link.href}
              aria-current={link.current ? "page" : undefined}
            >
              {link.label}
            </SiteLink>
          ))}
        </nav>
        <div className="mast-tools">
          <SearchButton label={t.search}>
            <Icon name="search" size={16} />
            <span className="label">{t.search}</span>
            <kbd>Ctrl K</kbd>
          </SearchButton>
          <a
            className="tool gh-stars"
            href={repo}
            aria-label={stars === undefined ? "GitHub" : "GitHub · " + stars + (en ? " stars" : " 个星标")}
          >
            <Icon name="github" size={16} />
            {stars !== undefined && (
              <span className="figures">
                <Icon name="star" size={12} />
                {compact(stars)}
              </span>
            )}
          </a>
          <ThemePicker label={t.theme} options={t.themeOptions} />
          {alternate && (
            <a className="tool lang" href={other} lang={en ? "zh-CN" : "en"} aria-label={t.language}>
              {en ? "中文" : "EN"}
            </a>
          )}
          <a className="btn is-small is-primary" href={site.release.files.installer.url}>
            {t.download}
          </a>
          <MastMenu label={menu}>
            {links.map((link) => (
              <SiteLink lang={lang} key={link.href} href={link.href}>
                {link.label}
              </SiteLink>
            ))}
            <a href={repo}>GitHub</a>
            {alternate && (
              <a href={other} lang={en ? "zh-CN" : "en"}>
                <Icon name="language" />
                {t.language}
              </a>
            )}
          </MastMenu>
        </div>
      </div>
    </header>
  );
}
