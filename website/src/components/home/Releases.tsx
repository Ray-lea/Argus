import { Icon } from "@/components/site/Icon";
import { SiteLink } from "@/components/site/SiteLink";
import { localized, repo, type Language, type ReleaseNote } from "@/lib/site";
import ReleasePoints from "./ReleasePoints";
import { copyFor, localize } from "@/lib/site-i18n";
import { dateLocale, languageInfo, notesLanguage } from "@/lib/languages";

// The latest releases from the changelog as a timeline: a summary of each and
// the names of what it added and fixed.
const words = {
  zh: { latest: "最新", withdrawn: "已撤回", all: "完整更新日志", releases: "GitHub Releases", show: (n: number) => `显示其余 ${n} 项`, hide: "收起" },
  en: { latest: "Latest", withdrawn: "Withdrawn", all: "Full changelog", releases: "GitHub Releases", show: (n: number) => `Show ${n} more`, hide: "Show less" },
};
export const longDate = (date: string, lang: Language) => {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(Date.UTC(y, m - 1, d)).toLocaleDateString(dateLocale(lang), { year: "numeric", month: "short", day: "numeric", timeZone: "UTC" });
};
// The intro, or else the first entry up to the end of its first sentence.
function summary(note: ReleaseNote, lang: Language) {
  if (note.intro) return note.intro;
  const first = note.groups.find((g) => g.items.length)?.items[0];
  if (!first) return "";
  const english = notesLanguage(lang) === "en";
  const sentence = english ? first.text.split(/(?<=\.)\s/)[0] : first.text.split(/(?<=。)/)[0];
  return first.label ? first.label + (english ? ": " : "：") + sentence : sentence;
}

export default function Releases({ notes, lang, count = 5 }: { notes: ReleaseNote[]; lang: Language; count?: number }) {
  const w = copyFor(lang, words),
    page = localized(lang, "/changelog/");
  return (
    <>
      {lang !== notesLanguage(lang) && <p className="release-language">{localize(lang, notesLanguage(lang) === "zh" ? "Release notes are shown in Simplified Chinese." : "Release notes are shown in English.")}</p>}
      <ol className="releases">
        {notes.slice(0, count).map((note, i) => {
          const named = note.groups
            .filter((g) => g.kind === "feature" || g.kind === "fix" || g.kind === "performance")
            .flatMap((g) => g.items.filter((item) => item.label).map((item) => ({ kind: g.kind, label: item.label })));
          const counted = note.groups.filter((g) => g.kind === "feature" || g.kind === "fix" || g.kind === "performance");
          return (
            <li key={note.version} className={"release" + (i === 0 ? " is-latest" : "") + (note.withdrawn ? " is-withdrawn" : "")} data-rise="">
              <div className="release-meta">
                <SiteLink lang={lang} className="release-tag figures" href={page + "#v" + note.version}>
                  v{note.version}
                </SiteLink>
                <time dateTime={note.date}>{longDate(note.date, lang)}</time>
              </div>
              <div className="release-body">
                {(i === 0 || note.withdrawn) && (
                  <span className={"release-badge" + (note.withdrawn ? " is-muted" : "")}>{note.withdrawn ? w.withdrawn : w.latest}</span>
                )}
                <p lang={languageInfo(notesLanguage(lang)).tag}>{summary(note, lang)}</p>
                {named.length > 0 && <ReleasePoints items={named} limit={6} show={w.show(named.length - 6)} hide={w.hide} />}
                {counted.length > 0 && (
                  <p className="release-counts figures">
                    {counted.map((g) => g.title + " " + g.items.length).join(" · ")}
                  </p>
                )}
              </div>
            </li>
          );
        })}
      </ol>
      <p className="release-more">
        <SiteLink lang={lang} className="text-link" href={page}>
          {w.all} <Icon name="arrow" size={14} />
        </SiteLink>
        <a className="text-link" href={repo + "/releases"}>
          {w.releases} <Icon name="external" size={14} />
        </a>
      </p>
    </>
  );
}
