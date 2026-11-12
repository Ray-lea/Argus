import { ViewTransition } from "react";
import { localized, repo, site, text, type Language } from "@/lib/site";
import { compact } from "@/components/home/OpenSource";
import { DownloadButton } from "./DownloadButton";
import { LanguageSwitch } from "./LanguageSwitch";
import { localize } from "@/lib/site-i18n";
import { MastFrame, MastMenu, SearchButton, ThemePicker } from "./HeaderTools";
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
    en = lang !== "zh";
  const links = [
    { href: "/docs/", label: t.docs, current: path.startsWith("/docs/") },
    { href: localized(lang, "/download/"), label: t.download, current: path === "/download/" },
    { href: localized(lang, "/changelog/"), label: t.changelog, current: path === "/changelog/" },
    { href: localized(lang, "/#faq"), label: t.questions, current: false },
  ];
  const stars = site.repository?.stars;
  const menu = localize(lang, en ? "Menu" : "菜单");
  return (
    <MastFrame wide={wide}>
      <div className="mast-inner">
        <SiteLink lang={lang} className="brand" href={localized(lang)}>
          <img src="/assets/argus.svg" width="22" height="22" alt="" />
          Argus
        </SiteLink>
        <nav className="mast-nav" aria-label={localize(lang, en ? "Main" : "主导航")}>
          {links.map((link) => (
            <SiteLink
              lang={lang}
              key={link.href}
              href={link.href}
              aria-current={link.current ? "page" : undefined}
            >
              {link.label}
              {link.current && (
                // Moving between pages, the mark slides from one link to the next.
                <ViewTransition name="mast-current" share="mast-current" default="none">
                  <i className="mast-current" aria-hidden="true" />
                </ViewTransition>
              )}
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
            aria-label={stars === undefined ? "GitHub" : "GitHub · " + stars + " " + localize(lang, en ? "stars" : "个星标")}
          >
            <span className="gh-mark">
              <Icon name="github" size={16} />
            </span>
            {stars !== undefined && (
              <span className="figures gh-count">
                <span className="gh-star">
                  <Icon name="star" size={12} />
                </span>
                {compact(stars)}
              </span>
            )}
          </a>
          <ThemePicker label={t.theme} options={t.themeOptions} />
          {alternate && (
            <>
              <LanguageSwitch lang={lang} path={path} label={t.language} />
              {/* Fetch the destination on intent; its scripts start only on navigation. */}
              <script
                type="speculationrules"
                dangerouslySetInnerHTML={{
                  __html: JSON.stringify({ prefetch: [{ source: "document", where: { selector_matches: ".language-option:not([aria-checked=true]), .language-offer-go" }, eagerness: "moderate" }] }),
                }}
              />
            </>
          )}
          <DownloadButton primary quiet className="is-small mast-get" href={site.release.files.installer.url} started={t.started}>
            {t.download}
          </DownloadButton>
          <MastMenu label={menu}>
            <SearchButton label={t.search}><Icon name="search" size={16} />{t.search}</SearchButton>
            {links.map((link) => (
              <SiteLink lang={lang} key={link.href} href={link.href}>
                {link.label}
              </SiteLink>
            ))}
            <a href={repo}>GitHub</a>
          </MastMenu>
        </div>
      </div>
    </MastFrame>
  );
}
