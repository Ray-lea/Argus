//! A changelog entry as the app lays it out (the update dialog and the About
//! page), after the website's changelog: a notice, an introduction, and the
//! changes grouped by kind under their headings, each change with its bold
//! label apart from its text. The headings are those of
//! `resources/changelog.json`, which `scripts/release.mjs` writes, in every
//! language of the changelog.

use argus_core::i18n::Locale;
use std::sync::OnceLock;

/// The changelog compiled into the app in `locale`: one file per language,
/// translated entry for entry from the Simplified Chinese one.
pub(super) fn source(locale: Locale) -> &'static str {
    match locale {
        Locale::ZhCn => include_str!("../../../changelog/zh-CN.md"),
        Locale::ZhTw => include_str!("../../../changelog/zh-TW.md"),
        Locale::EnUs => include_str!("../../../changelog/en-US.md"),
        Locale::EsEs => include_str!("../../../changelog/es-ES.md"),
        Locale::DeDe => include_str!("../../../changelog/de-DE.md"),
        Locale::JaJp => include_str!("../../../changelog/ja-JP.md"),
        Locale::FrFr => include_str!("../../../changelog/fr-FR.md"),
        Locale::RuRu => include_str!("../../../changelog/ru-RU.md"),
    }
}

/// What a group of changes is, for its colour and its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Feature,
    Fix,
    Performance,
    Breaking,
    Upgrade,
    Known,
    Other,
}

/// The kinds told apart, by their key in `resources/changelog.json`; any
/// other heading (reverts, included releases...) is `Other`.
const KINDS: [(&str, Kind); 6] = [
    ("feature", Kind::Feature),
    ("fix", Kind::Fix),
    ("performance", Kind::Performance),
    ("breaking", Kind::Breaking),
    ("upgrade", Kind::Upgrade),
    ("known", Kind::Known),
];

/// The headings of each kind in every language, in lower case (matched,
/// never shown).
fn headings() -> &'static [(Kind, Vec<String>)] {
    static HEADINGS: OnceLock<Vec<(Kind, Vec<String>)>> = OnceLock::new();
    HEADINGS.get_or_init(|| {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../../resources/changelog.json"))
                .expect("changelog headings");
        KINDS
            .iter()
            .map(|(key, kind)| {
                let names = conf["sections"][key]
                    .as_object()
                    .into_iter()
                    .flat_map(|languages| languages.values())
                    .filter_map(|names| names.as_array())
                    .flatten()
                    .filter_map(|name| name.as_str())
                    .map(str::to_lowercase)
                    .collect();
                (*kind, names)
            })
            .collect()
    })
}

impl Kind {
    fn of(title: &str) -> Self {
        let title = title.trim().to_lowercase();
        headings()
            .iter()
            .find(|(_, names)| names.contains(&title))
            .map_or(Self::Other, |(kind, _)| *kind)
    }
}

/// One change: its bold label (`**Label**: text`), if it has one, and its
/// text in Markdown (links, bold, code).
#[derive(Debug, PartialEq)]
pub(super) struct Item {
    pub label: Option<String>,
    pub body: String,
}

#[derive(Debug, PartialEq)]
pub(super) struct Group {
    pub kind: Kind,
    /// The heading as written, in the changelog's language.
    pub title: String,
    pub items: Vec<Item>,
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Note {
    /// A paragraph that is bold throughout, such as a withdrawn release's.
    pub notice: Option<String>,
    /// The paragraphs before the first group.
    pub intro: Vec<String>,
    pub groups: Vec<Group>,
}

impl Note {
    /// The number of changes.
    pub fn changes(&self) -> usize {
        self.groups.iter().map(|group| group.items.len()).sum()
    }
}

/// Splits `**Label**：text` (or `: text`) into its label and text.
fn item(text: &str) -> Item {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix("**")
        && let Some(end) = rest.find("**")
    {
        let after = &rest[end + 2..];
        if let Some(body) = after.strip_prefix('：').or_else(|| after.strip_prefix(':')) {
            return Item {
                label: Some(rest[..end].trim().to_owned()),
                body: body.trim().to_owned(),
            };
        }
    }
    Item {
        label: None,
        body: text.to_owned(),
    }
}

/// The parts of a changelog entry's body.
pub(super) fn parse(notes: &str) -> Note {
    let mut note = Note::default();
    let mut paragraph = String::new();
    // The lines of the change being read: an item continues on indented lines.
    let mut change: Option<String> = None;
    let finish_change = |change: &mut Option<String>, note: &mut Note| {
        if let Some(text) = change.take() {
            if note.groups.is_empty() {
                note.groups.push(Group {
                    kind: Kind::Other,
                    title: String::new(),
                    items: Vec::new(),
                });
            }
            note.groups.last_mut().unwrap().items.push(item(&text));
        }
    };
    let finish_paragraph = |paragraph: &mut String, note: &mut Note| {
        let text = std::mem::take(paragraph).trim().to_owned();
        if text.is_empty() {
            return;
        }
        let bold = text
            .strip_prefix("**")
            .and_then(|t| t.strip_suffix("**"))
            .filter(|inner| !inner.contains("**"));
        match bold {
            Some(inner) if note.notice.is_none() && note.groups.is_empty() => {
                note.notice = Some(inner.trim().to_owned());
            }
            _ if note.groups.is_empty() => note.intro.push(text),
            // Text between changes reads as a change of its own.
            _ => note.groups.last_mut().unwrap().items.push(item(&text)),
        }
    };
    for line in notes.lines() {
        let trimmed = line.trim();
        if let Some(title) = trimmed.strip_prefix("### ") {
            finish_change(&mut change, &mut note);
            finish_paragraph(&mut paragraph, &mut note);
            note.groups.push(Group {
                kind: Kind::of(title),
                title: title.trim().to_owned(),
                items: Vec::new(),
            });
        } else if let Some(text) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            finish_change(&mut change, &mut note);
            finish_paragraph(&mut paragraph, &mut note);
            change = Some(text.to_owned());
        } else if trimmed.is_empty() {
            finish_change(&mut change, &mut note);
            finish_paragraph(&mut paragraph, &mut note);
        } else if let Some(text) = change.as_mut() {
            text.push(' ');
            text.push_str(trimmed);
        } else {
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(trimmed);
        }
    }
    finish_change(&mut change, &mut note);
    finish_paragraph(&mut paragraph, &mut note);
    note.groups.retain(|group| !group.items.is_empty());
    note
}

#[cfg(test)]
mod tests {
    use super::{Item, Kind, Locale, Note, headings, parse, source};

    #[test]
    fn groups_changes_under_their_headings() {
        let note = parse(
            "**此版本已撤回。**\n\n本版新增界面缩放。\n\n### 新功能\n\n- **界面缩放**：自定义比例。\n- **字体大小**: 可调整，\n  立即生效。\n\n### Fixes\n\n- Plain change ([#20](https://example.com))\n\n### 包含的更新\n\n- 包含 v0.3.1 的修复。\n",
        );
        assert_eq!(note.notice.as_deref(), Some("此版本已撤回。"));
        assert_eq!(note.intro, ["本版新增界面缩放。"]);
        let kinds = note.groups.iter().map(|g| g.kind).collect::<Vec<_>>();
        assert_eq!(kinds, [Kind::Feature, Kind::Fix, Kind::Other]);
        assert_eq!(
            note.groups[0].items[1],
            Item {
                label: Some("字体大小".into()),
                body: "可调整， 立即生效。".into(),
            }
        );
        assert_eq!(note.groups[1].items[0].label, None);
        assert_eq!(note.changes(), 4);
    }

    #[test]
    fn a_bold_phrase_inside_text_is_no_label() {
        let note = parse("### 修复\n\n- 修复 **某处** 的问题。\n");
        assert_eq!(note.groups[0].items[0].label, None);
        assert_eq!(note.groups[0].items[0].body, "修复 **某处** 的问题。");
    }

    #[test]
    fn headings_of_every_language_have_their_kind() {
        for (title, kind) in [
            ("升级提示", Kind::Upgrade),
            ("Upgrading", Kind::Upgrade),
            ("Hinweise zur Aktualisierung", Kind::Upgrade),
            ("已知限制", Kind::Known),
            ("Known issues", Kind::Known),
            ("既知の問題", Kind::Known),
            ("破坏性变更", Kind::Breaking),
            ("Performance", Kind::Performance),
            ("Производительность", Kind::Performance),
            ("修正", Kind::Fix),
            ("Nouvelles fonctionnalités", Kind::Feature),
            ("Cambios incluidos", Kind::Other),
        ] {
            let note = parse(&format!("### {title}\n\n- x\n"));
            assert_eq!(note.groups[0].kind, kind, "{title}");
        }
        // A heading names one kind in every language.
        let all = headings()
            .iter()
            .flat_map(|(kind, names)| names.iter().map(move |name| (name, kind)));
        for (name, kind) in all.clone() {
            assert!(
                all.clone().all(|(other, k)| other != name || k == kind),
                "{name}"
            );
        }
    }

    /// Entries split at their headings: the heading line and the body.
    fn entries(source: &str) -> Vec<(&str, &str)> {
        source
            .split("\n## ")
            .skip(1)
            .map(|entry| entry.split_once('\n').unwrap_or((entry, "")))
            .collect()
    }

    #[test]
    fn every_entry_of_the_changelogs_has_changes() {
        for locale in Locale::ALL {
            for (heading, body) in entries(source(locale)) {
                assert!(parse(body).changes() > 0, "{locale:?}: {heading}");
            }
        }
    }

    /// The app lays every language out as it does the source: the same
    /// releases, sections of the same kinds and the same changes, labelled
    /// where the source's are.
    #[test]
    fn translations_match_the_source_entry_for_entry() {
        let shape = |note: &Note| {
            let groups = note.groups.iter().map(|group| {
                let labels = group
                    .items
                    .iter()
                    .map(|item| item.label.is_some())
                    .collect::<Vec<_>>();
                (group.kind, labels)
            });
            (
                note.notice.is_some(),
                note.intro.len(),
                groups.collect::<Vec<_>>(),
            )
        };
        let original = entries(source(Locale::SOURCE));
        for locale in Locale::ALL {
            let translated = entries(source(locale));
            let headings =
                |list: &[(&str, &str)]| list.iter().map(|(h, _)| h.to_string()).collect::<Vec<_>>();
            assert_eq!(headings(&translated), headings(&original), "{locale:?}");
            for ((heading, body), (_, theirs)) in translated.iter().zip(&original) {
                assert_eq!(
                    shape(&parse(body)),
                    shape(&parse(theirs)),
                    "{locale:?}: {heading}"
                );
            }
        }
    }
}
