import { longDate } from "@/components/home/Releases";
import { Icon } from "@/components/site/Icon";
import { SiteFrame } from "@/components/site/SiteFrame";
import { VersionNav } from "@/components/site/VersionNav";
import { pageMetadata } from "@/lib/page";
import { repo, site, type Language, type ReleaseNote } from "@/lib/site";

// A release's notice, intro and groups of entries; new features as cards.
export function ReleaseBody({ note }: { note: ReleaseNote }) {
  return (
    <>
      {note.notice && (
        <p className="cl-notice">
          <Icon name="alert" size={16} />
          {note.notice}
        </p>
      )}
      {note.introHtml && <p className="cl-intro" dangerouslySetInnerHTML={{ __html: note.introHtml }} />}
      {note.groups.map((group) => (
        <section key={group.title} className={"cl-group is-" + group.kind}>
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
    </>
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
    facts: (count: number, first: string) => [`${count} releases`, `First released ${first}`],
  },
};

export const changelogMetadata = (lang: Language) =>
  pageMetadata({ lang, path: "/changelog/", title: copy[lang].title, description: copy[lang].description });

export function Changelog({ lang }: { lang: Language }) {
  const t = copy[lang],
    notes = site.changelog[lang],
    r = site.release;
  const first = notes.at(-1);
  return (
    <SiteFrame lang={lang} path="/changelog/" title={t.title}>
      <div className="site-wrap changelog">
        <header className="cl-head">
          <h1 data-rise="">{t.h1}</h1>
          <ul className="cl-facts figures" data-rise="2">
            {t.facts(notes.length, first ? longDate(first.date, lang) : "").map((fact) => (
              <li key={fact}>{fact}</li>
            ))}
          </ul>
          <div className="cl-actions" data-rise="3">
            <a className="btn is-primary" href={r.files.installer.url}>
              <Icon name="download" size={18} />
              {t.download} {r.tag}
            </a>
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
            }))}
          />
          <div className="cl-list">
            {notes.map((note, i) => (
              <article key={note.version} id={"v" + note.version} className={"cl-release" + (note.withdrawn ? " is-withdrawn" : "")}>
                <header className="cl-release-head">
                  <h2>
                    <a href={"#v" + note.version} className="figures">
                      v{note.version}
                    </a>
                  </h2>
                  {i === 0 && <span className="release-badge">{t.latest}</span>}
                  {note.withdrawn && <span className="release-badge is-muted">{t.withdrawn}</span>}
                  <time dateTime={note.date}>{longDate(note.date, lang)}</time>
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
                <ReleaseBody note={note} />
              </article>
            ))}
          </div>
        </div>
      </div>
    </SiteFrame>
  );
}
