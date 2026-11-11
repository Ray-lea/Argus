import { copyFor, localize } from "@/lib/site-i18n";
import { longDate } from "@/components/home/Releases";
import { DownloadButton } from "@/components/site/DownloadButton";
import { CopyButton } from "@/components/site/CopyButton";
import { Icon } from "@/components/site/Icon";
import { KindFilter } from "@/components/site/KindFilter";
import { SiteFrame } from "@/components/site/SiteFrame";
import { VersionNav } from "@/components/site/VersionNav";
import { pageMetadata } from "@/lib/page";
import { changelogLanguage, localized, origin, repo, site, text, type Language, type ReleaseNote } from "@/lib/site";
import { languageInfo } from "@/lib/languages";

// A release's notice, intro and groups of entries; new features as cards.
export function ReleaseBody({ note, lang }: { note: ReleaseNote; lang: Language }) {
  return (
    <div lang={languageInfo(changelogLanguage(lang)).tag} className="release-content">
      {note.notice && (
        <p className="cl-notice">
          <Icon name="alert" size={16} />
          {note.notice}
        </p>
      )}
      {note.introHtml && <p className="cl-intro" dangerouslySetInnerHTML={{ __html: note.introHtml }} />}
      {note.groups.map((group) => (
        <section key={group.title} className={"cl-group is-" + group.kind} data-rise="">
          <h3>
            <span className="cl-kind">{group.title}</span>
            <small className="figures">{group.items.length}</small>
          </h3>
          <ul className={group.kind === "feature" ? "cl-cards" : "cl-items"}>
            {group.items.map((item, j) => (
              <li key={j}>
                {item.label && <b>{item.label}</b>}
                <span dangerouslySetInnerHTML={{ __html: item.html }} />
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

// Every release from the changelog files, newest first: the version index on
// the side, each release with its intro and its groups of entries.
const copy = {
  zh: {
    title: "Argus 更新日志",
    description: "Argus 各版本的新功能、修复、性能改进与升级提示，取自项目的更新日志。",
    h1: "更新日志",
    download: "下载",
    rss: "订阅发布",
    releases: "GitHub Releases",
    index: "版本",
    latest: "最新",
    withdrawn: "已撤回",
    page: "发布页",
    compare: "与上一版比较",
    filter: "按类别筛选",
    all: "全部",
    copyLink: "复制此版本的链接",
    copied: "已复制链接",
    facts: (count: number, first: string) => [`${count} 个版本`, `首个版本发布于 ${first}`],
  },
  en: {
    title: "Argus changelog",
    description: "New features, fixes, performance work and upgrade notes in each Argus release, from the project's changelog.",
    h1: "Changelog",
    download: "Download",
    rss: "Subscribe",
    releases: "GitHub Releases",
    index: "Versions",
    latest: "Latest",
    withdrawn: "Withdrawn",
    page: "Release page",
    compare: "Compare with previous",
    filter: "Filter by kind",
    all: "All",
    copyLink: "Copy a link to this release",
    copied: "Link copied",
    facts: (count: number, first: string) => [`${count} releases`, `First released ${first}`],
  },
};

export const changelogMetadata = (lang: Language) =>
  pageMetadata({ lang, path: "/changelog/", title: copyFor(lang, copy).title, description: copyFor(lang, copy).description });

export function Changelog({ lang }: { lang: Language }) {
  const t = copyFor(lang, copy),
    notes = site.changelog[lang],
    r = site.release;
  const first = notes.at(-1);
  // The kinds of entries in any release, new features first, with their
  // titles and how many entries each has across all releases.
  const kinds = new Map<string, { kind: string; title: string; count: number }>();
  for (const note of notes)
    for (const group of note.groups) {
      const known = kinds.get(group.kind) ?? { kind: group.kind, title: group.title, count: 0 };
      known.count += group.items.length;
      kinds.set(group.kind, known);
    }
  const order = ["feature", "fix", "performance", "breaking", "upgrade", "known", "other"];
  const options = [...kinds.values()].sort((a, b) => order.indexOf(a.kind) - order.indexOf(b.kind));
  const total = options.reduce((sum, k) => sum + k.count, 0);
  const kindsOf = (note: ReleaseNote) => [...new Set(note.groups.map((g) => g.kind))].join(" ");
  return (
    <SiteFrame lang={lang} path="/changelog/" title={t.title}>
      <div className="site-wrap changelog">
        <header className="cl-head">
          <h1>{t.h1}</h1>
          {lang !== changelogLanguage(lang) && <p className="release-language">{localize(lang, changelogLanguage(lang) === "zh" ? "Release notes are shown in Simplified Chinese." : "Release notes are shown in English.")}</p>}
          <ul className="cl-facts figures">
            {t.facts(notes.length, first ? longDate(first.date, lang) : "").map((fact) => (
              <li key={fact}>{fact}</li>
            ))}
          </ul>
          <div className="cl-actions">
            <DownloadButton primary href={r.files.installer.url} started={text[lang].started}>
              {t.download} {r.tag}
            </DownloadButton>
            <a className="btn" href={repo + "/releases.atom"}>
              <Icon name="rss" size={16} />
              {t.rss}
            </a>
            <a className="btn" href={repo + "/releases"}>
              <Icon name="github" size={16} />
              {t.releases}
            </a>
          </div>
        </header>

        <div className="cl-body">
          <VersionNav
            label={t.index}
            versions={notes.map((note) => ({
              id: "v" + note.version,
              version: note.version,
              date: longDate(note.date, lang),
              note: note.withdrawn ? t.withdrawn : undefined,
              kinds: kindsOf(note),
            }))}
          />
          <div className="cl-main">
          <KindFilter label={t.filter} options={[{ kind: "", title: t.all, count: total }, ...options]} />
          <div className="cl-list">
            {notes.map((note, i) => (
              <article
                key={note.version}
                id={"v" + note.version}
                className={"cl-release" + (note.withdrawn ? " is-withdrawn" : "")}
                data-kinds={kindsOf(note)}
              >
                <header className="cl-release-head" data-rise="">
                  <h2>
                    <a href={"#v" + note.version} className="figures">
                      v{note.version}
                    </a>
                  </h2>
                  {i === 0 && <span className="release-badge">{t.latest}</span>}
                  {note.withdrawn && <span className="release-badge is-muted">{t.withdrawn}</span>}
                  <time dateTime={note.date}>{longDate(note.date, lang)}</time>
                  <CopyButton
                    className="cl-copy"
                    icon="link"
                    text={origin + localized(lang, "/changelog/") + "#v" + note.version}
                    label={t.copyLink}
                    done={t.copied}
                  />
                  <span className="cl-links">
                    <a href={repo + "/releases/tag/v" + note.version}>
                      {t.page} <Icon name="external" size={13} />
                    </a>
                    {note.compare && (
                      <a href={note.compare}>
                        {t.compare} <Icon name="external" size={13} />
                      </a>
                    )}
                  </span>
                </header>
                <ReleaseBody note={note} lang={lang} />
              </article>
            ))}
          </div>
          </div>
        </div>
      </div>
    </SiteFrame>
  );
}
