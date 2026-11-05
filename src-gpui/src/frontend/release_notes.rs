//! A changelog entry as the app lays it out (the update dialog and the About
//! page), after the website's changelog: a notice, an introduction, and the
//! changes grouped by kind under their headings, each change with its bold
//! label apart from its text. The headings are those `scripts/release.mjs`
//! writes and `scripts/lib/release-notes.ts` reads, in either language.

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

/// The headings of each kind, in both changelogs (matched, never shown).
#[rustfmt::skip]
const HEADINGS: [(Kind, &[&str]); 6] = [
    (Kind::Feature, &["新功能", "功能", "features", "new features"]), // i18n-ignore
    (Kind::Fix, &["修复", "fixes", "bug fixes"]), // i18n-ignore
    (Kind::Performance, &["性能", "performance"]), // i18n-ignore
    (Kind::Breaking, &["破坏性变更", "breaking changes"]), // i18n-ignore
    (Kind::Upgrade, &["升级提示", "upgrading", "upgrade notes"]), // i18n-ignore
    (Kind::Known, &["已知问题", "已知限制", "known issues", "known limitations"]), // i18n-ignore
];

impl Kind {
    fn of(title: &str) -> Self {
        let title = title.trim().to_lowercase();
        HEADINGS
            .iter()
            .find(|(_, names)| names.contains(&title.as_str()))
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
    use super::{Item, Kind, parse};

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
    fn upgrade_and_known_headings_in_both_languages() {
        for (title, kind) in [
            ("升级提示", Kind::Upgrade),
            ("Upgrading", Kind::Upgrade),
            ("已知限制", Kind::Known),
            ("Known issues", Kind::Known),
            ("破坏性变更", Kind::Breaking),
            ("Performance", Kind::Performance),
        ] {
            let note = parse(&format!("### {title}\n\n- x\n"));
            assert_eq!(note.groups[0].kind, kind, "{title}");
        }
    }

    #[test]
    fn every_entry_of_the_changelogs_has_changes() {
        for source in [
            include_str!("../../../CHANGELOG.md"),
            include_str!("../../../CHANGELOG.en.md"),
        ] {
            for entry in source.split("\n## ").skip(1) {
                let body = entry.split_once('\n').map_or("", |(_, body)| body);
                assert!(
                    parse(body).changes() > 0,
                    "{}",
                    entry.lines().next().unwrap()
                );
            }
        }
    }
}
