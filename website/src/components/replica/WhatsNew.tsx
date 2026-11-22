/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useEffect, useState, type CSSProperties, type MouseEvent } from "react";
import { notesLanguage } from "@/lib/languages";
import { language } from "./i18n";
import { Icon, Logo } from "./icons";
import { appVersion } from "./model";

// One release as the site's changelog reads it (scripts/content.mjs
// `releaseNotes`, written per language as public/assets/release-notes-<id>.json).
interface Note {
  version: string;
  date: string;
  notice: string;
  introHtml: string;
  groups: { title: string; kind: string; items: { label: string; html: string }[] }[];
}
const repo = "https://github.com/Ray-lea/Argus";

// This version's notes, in the site's language or else its source language,
// as the changelog page shows them.
function useNotes() {
  const [note, setNote] = useState<Note | null | undefined>(undefined);
  useEffect(() => {
    let live = true;
    const get = (id: string) =>
      fetch(`/assets/release-notes-${id}.json`).then((r) => (r.ok ? (r.json() as Promise<Note>) : Promise.reject(r.status)));
    const lang = language();
    get(lang)
      .catch(() => (notesLanguage(lang) === lang ? Promise.reject() : get(notesLanguage(lang))))
      .then((n) => live && setNote(n.version === appVersion ? n : null))
      .catch(() => live && setNote(null));
    return () => {
      live = false;
    };
  }, []);
  return note;
}

// The parts come in one after another, 60 ms apart (the first eight), from
// 16 px below over 560 ms (welcome.rs `rise`).
const rise = (order: number) => ({ "--rise": Math.min(order, 8) } as CSSProperties);

// "Argus 0.5.0 的更新内容" (welcome.rs `update_dialog`, as the app shows the
// version it runs): the release's notice, introduction and changes by kind,
// new features as cards in two columns, the others as a list.
export default function WhatsNew({ onClose }: { onClose: () => void }) {
  const note = useNotes();
  // Links in the notes leave the demo in a tab of their own.
  const follow = (event: MouseEvent) => {
    const link = (event.target as Element).closest("a");
    if (!link) return;
    event.preventDefault();
    window.open(link.href, "_blank", "noopener");
  };
  let order = 3;
  return (
    <div className="a-dialog is-whatsnew" role="dialog" aria-modal="true" aria-labelledby="a-dialog-title" tabIndex={-1}>
      <div className="a-dialog-body a-scroll">
        <header className="a-whatsnew-head" data-enter="" style={rise(0)}>
          <Logo size={40} />
          <div>
            <h3 id="a-dialog-title">{`Argus ${appVersion} 的更新内容`}</h3>
            {note && <p>{`${note.date}发布`}</p>}
          </div>
        </header>
        {note === null && (
          <p className="a-whatsnew-empty" data-enter="" style={rise(3)}>
            暂无可显示的更新内容，可在 GitHub 上查看。
          </p>
        )}
        {note && (
          <div className="a-whatsnew-notes" onClick={follow}>
            {note.notice && (
              <p className="a-whatsnew-notice" data-enter="" style={rise(order++)}>
                <Icon name="warning" size={14} />
                <span translate="no">{note.notice}</span>
              </p>
            )}
            {note.introHtml && <p className="a-whatsnew-intro" data-enter="" style={rise(order++)} dangerouslySetInnerHTML={{ __html: note.introHtml }} />}
            {note.groups.map((group) => {
              const cards = group.kind === "feature" && group.items.every((item) => item.label);
              return (
                <section key={group.title} className={"a-whatsnew-group is-" + group.kind} data-enter="" style={rise(order++)}>
                  <span className="a-kind-tag">
                    <i />
                    <b translate="no">{group.title}</b>
                    <small>{group.items.length}</small>
                  </span>
                  <ul className={cards ? "a-whatsnew-cards" : "a-whatsnew-items"}>
                    {group.items.map((item, i) => (
                      <li key={i}>
                        {cards ? (
                          <>
                            <b translate="no">{item.label}</b>
                            <span dangerouslySetInnerHTML={{ __html: item.html }} />
                          </>
                        ) : (
                          <span>
                            {item.label && <b translate="no">{item.label} </b>}
                            <span dangerouslySetInnerHTML={{ __html: item.html }} />
                          </span>
                        )}
                      </li>
                    ))}
                  </ul>
                </section>
              );
            })}
          </div>
        )}
      </div>
      <div className="a-dialog-foot">
        <a className="a-btn is-ghost a-whatsnew-github" href={`${repo}/releases/tag/v${appVersion}`} target="_blank" rel="noopener">
          <Icon name="external" />
          在 GitHub 上查看
        </a>
        <button type="button" className="a-btn is-accent is-wide" onClick={onClose} data-primary>
          关闭
        </button>
      </div>
    </div>
  );
}
