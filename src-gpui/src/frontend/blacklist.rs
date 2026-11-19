//! The process blacklist: processes the user never wants running.
//!
//! The details pane adds the selected process's full path; the blacklist page
//! lists the entries and removes them. While the list is not empty, every
//! sample is swept off the UI thread: any process that matches an entry is
//! terminated at once, so bundleware and its helpers die as they appear.
//!
//! Matching is exact, case-insensitive, and never a substring: an entry that
//! holds a path separator (`\` or `/`) equals the process's full path, any
//! other entry equals the image name (`notepad` reads as `notepad.exe`). A
//! fragment like `360` can therefore only ever mean `360.exe`. Sweeps rate
//! limit per entry, so a guarded process that keeps respawning cannot turn
//! into a kill–resample–kill storm.
use super::model::{text, values};
use super::prefs::Prefs;
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The preference key the entries live under.
pub(in crate::frontend) const KEY: &str = "processBlacklist";
/// The quiet time between two sweeps that kill under one entry: shorter than
/// a guarded process's restart takes, long enough to break the storm.
const COOLDOWN: Duration = Duration::from_secs(10);

/// One blacklisted entry, normalized for comparison: lowercased, and either
/// a full path (kept as written, separators aside) or an image name that
/// gains `.exe` when written without an extension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::frontend) enum Entry {
    Image(String),
    Path(String),
}

impl Entry {
    fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        let lowered = raw.to_lowercase();
        // A path: anything with a separator or a drive letter. Comparison
        // normalizes the separators the way `format!` prints them.
        if lowered.contains('\\') || lowered.contains('/') || lowered.ends_with(':') {
            let path = lowered.replace('/', "\\");
            let path = path.trim_end_matches('\\');
            if path.is_empty() {
                return None;
            }
            return Some(Self::Path(path.to_owned()));
        }
        Some(Self::Image(if raw.contains('.') {
            lowered
        } else {
            format!("{lowered}.exe")
        }))
    }

    /// The entry as stored in the preference list.
    fn stored(&self) -> String {
        match self {
            Self::Image(name) => name.clone(),
            Self::Path(path) => path.clone(),
        }
    }
}

/// The entries, in the order added, as shown on the page.
pub(in crate::frontend) fn names(prefs: &Prefs) -> Vec<String> {
    prefs
        .get::<Vec<String>>(KEY, vec![])
        .iter()
        .filter_map(|raw| Entry::parse(raw).map(|entry| entry.stored()))
        .collect()
}

/// Adds one entry; returns the stored form, or `None` for blank text or an
/// entry already present.
pub(in crate::frontend) fn add(prefs: &mut Prefs, raw: &str) -> Option<String> {
    let entry = Entry::parse(raw)?;
    let stored = entry.stored();
    let mut list = names(prefs);
    if list.contains(&stored) {
        return None;
    }
    list.push(stored.clone());
    prefs.set(KEY, &list);
    Some(stored)
}

/// Removes one entry, in the form it was added (`notepad` removes
/// `notepad.exe`, `d:/nfp.exe` removes `d:\nfp.exe`).
pub(in crate::frontend) fn remove(prefs: &mut Prefs, raw: &str) {
    let Some(entry) = Entry::parse(raw) else {
        return;
    };
    let stored = entry.stored();
    let list = names(prefs);
    let kept: Vec<String> = list
        .into_iter()
        .filter(|existing| *existing != stored)
        .collect();
    prefs.set(KEY, &kept);
}

/// Whether an entry is already blacklisted, in the form it would be stored
/// (`d:/nfp.exe` and `d:\nfp.exe` are the same path entry). The details
/// pane's toggle asks this.
pub(in crate::frontend) fn holds(prefs: &Prefs, raw: &str) -> bool {
    Entry::parse(raw).is_some_and(|entry| names(prefs).contains(&entry.stored()))
}

/// One process a sweep is to terminate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::frontend) struct Target {
    pub pid: u32,
    pub create: i64,
    /// The image name, for the log.
    pub name: String,
    /// The entry that matched, for the cooldown.
    pub entry: String,
}

/// The processes of a sample the blacklist asks to terminate. A creation
/// time below 0 marks an identity still pending, which waits for a later
/// sample, as the end action does.
pub(in crate::frontend) fn targets(
    snapshot: &Value,
    prefs: &Prefs,
    own_image: &str,
) -> Vec<Target> {
    let entries: Vec<Entry> = names(prefs)
        .iter()
        .filter_map(|raw| Entry::parse(raw))
        .collect();
    let own = Entry::parse(own_image);
    values(&snapshot["processes"])
        .iter()
        .filter_map(|p| {
            let pid = p["pid"].as_u64()?;
            let create = p["createTime"].as_f64()?;
            if create < 0. {
                return None;
            }
            let (pid, create) = (u32::try_from(pid).ok()?, create as i64);
            let image = text(p, "name").to_lowercase();
            // Argus never blacklists itself, whatever the list holds.
            if matches!(&own, Some(Entry::Image(name)) if *name == image) {
                return None;
            }
            let path = text(p, "path").to_lowercase().replace('/', "\\");
            let matched = entries.iter().find(|entry| match entry {
                Entry::Image(name) => *name == image,
                Entry::Path(entry) => !path.is_empty() && *entry == path,
            })?;
            Some(Target {
                pid,
                create,
                name: text(p, "name").to_owned(),
                entry: matched.stored(),
            })
        })
        .collect()
}

/// The targets still inside their entry's cooldown are left for later.
pub(in crate::frontend) fn cooled(
    targets: Vec<Target>,
    cooldown: &mut HashMap<String, Instant>,
) -> Vec<Target> {
    let now = Instant::now();
    let mut kept = Vec::new();
    for target in targets {
        let ready = cooldown
            .get(&target.entry)
            .is_none_or(|at| now.duration_since(*at) >= COOLDOWN);
        if ready {
            cooldown.insert(target.entry.clone(), now);
            kept.push(target);
        }
    }
    cooldown.retain(|_, at| now.duration_since(*at) < COOLDOWN * 2);
    kept
}

/// Sweeps one sample: terminates the targets off the UI thread. A plain
/// `terminate` rather than the end action's command, so a kill does not
/// request another sample: sampling continues at its own pace, and a
/// guarded process that respawns meets the next sweep, not a tight loop. A
/// fixture never reaches real processes, so its samples are left alone.
pub(in crate::frontend) fn sweep(host: &crate::native_runtime::Host, targets: &[Target]) {
    if targets.is_empty() {
        return;
    }
    let host = host.clone();
    let targets = targets.to_vec();
    std::thread::spawn(move || {
        for target in targets {
            let result = crate::dispatch::invoke(
                &host,
                "kill_process_quiet",
                serde_json::json!({"pid":target.pid,"createTime":target.create,"force":false}),
            );
            match result {
                Ok(_) => log::info!(
                    "blacklist: terminated {} (pid {}, entry {})",
                    target.name,
                    target.pid,
                    target.entry
                ),
                Err(error) => log::warn!(
                    "blacklist: {} (pid {}) survived: {}",
                    target.name,
                    target.pid,
                    error
                ),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    // Not the glob: the frontend's imports bring GPUI's own `test` attribute.
    use super::{Entry, Target, add, cooled, holds, names, remove, targets};
    use crate::frontend::prefs::Prefs;
    use serde_json::json;
    use std::collections::HashMap;

    fn store() -> Prefs {
        Prefs::load(
            std::env::temp_dir().join(format!("argus-blacklist-test-{}.json", std::process::id())),
        )
    }

    #[test]
    fn entries_normalize_and_paths_classify() {
        assert_eq!(
            Entry::parse(" 360SE.EXE ").unwrap(),
            Entry::Image("360se.exe".into())
        );
        assert_eq!(
            Entry::parse("Notepad").unwrap(),
            Entry::Image("notepad.exe".into())
        );
        assert_eq!(Entry::parse("  "), None);
        assert!(Entry::parse("").is_none());
        // Separators and drive letters make a path entry, whatever the
        // separators look like.
        assert_eq!(
            Entry::parse("D:/Tools/NFP.EXE").unwrap(),
            Entry::Path("d:\\tools\\nfp.exe".into())
        );
        assert_eq!(
            Entry::parse(r"d:\tools\nfp.exe\").unwrap(),
            Entry::Path("d:\\tools\\nfp.exe".into())
        );
        assert_eq!(Entry::parse("C:").unwrap(), Entry::Path("c:".into()));
    }

    #[test]
    fn add_is_case_insensitive_and_deduplicates() {
        let mut prefs = store();
        assert_eq!(add(&mut prefs, "wxwork.exe").as_deref(), Some("wxwork.exe"));
        assert_eq!(add(&mut prefs, " QQ ").as_deref(), Some("qq.exe"));
        assert_eq!(add(&mut prefs, "WXWORK.EXE"), None);
        assert_eq!(
            add(&mut prefs, "d:/nfp.exe").as_deref(),
            Some("d:\\nfp.exe")
        );
        assert_eq!(names(&prefs), ["wxwork.exe", "qq.exe", "d:\\nfp.exe"]);
        // Short form removes the stored extension form; either slash removes
        // the path entry.
        remove(&mut prefs, "QQ");
        remove(&mut prefs, "d:\\nfp.exe");
        assert_eq!(names(&prefs), ["wxwork.exe"]);
        assert!(!holds(&prefs, "qq.exe"));
    }

    #[test]
    fn a_name_entry_matches_never_a_substring_and_paths_match_the_full_path() {
        let mut prefs = store();
        add(&mut prefs, "360");
        let snapshot = json!({"processes":[
            {"pid":10,"createTime":1.,"name":"360SE.EXE","path":"C:/360/360SE.EXE"},
            {"pid":20,"createTime":2.,"name":"360.exe","path":"D:/360/360.exe"}
        ]});
        // `360` reads as `360.exe`: only that exact image name matches, and a
        // name entry ignores the path.
        let found = targets(&snapshot, &prefs, "argus.exe");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].pid, 20);
        assert_eq!(found[0].name, "360.exe");
        // The full path kills exactly that file, wherever its name runs.
        add(&mut prefs, "c:/360/360se.exe");
        let found = targets(&snapshot, &prefs, "argus.exe");
        assert_eq!(found.iter().map(|t| t.pid).collect::<Vec<_>>(), [10, 20]);
        // Pending identities and Argus itself are left alone.
        add(&mut prefs, "explorer.exe");
        let snapshot = json!({"processes":[
            {"pid":30,"createTime":-1.,"name":"explorer.exe","path":"C:/Windows/explorer.exe"},
            {"pid":40,"createTime":4.,"name":"argus.exe","path":"D:/argus.exe"}
        ]});
        assert!(targets(&snapshot, &prefs, "argus.exe").is_empty());
    }

    #[test]
    fn holds_accepts_either_slash_and_the_stored_form() {
        let mut prefs = store();
        add(&mut prefs, "d:/tools/nfp.exe");
        assert!(holds(&prefs, r"d:\TOOLS\NFP.EXE"));
        assert!(holds(&prefs, "d:/tools/nfp.exe"));
        // A same-named file elsewhere is a different entry.
        assert!(!holds(&prefs, r"c:\tools\nfp.exe"));
        // A name entry toggles through both spellings.
        add(&mut prefs, "wxwork");
        assert!(holds(&prefs, "WXWORK.EXE"));
        assert!(!holds(&prefs, "qq.exe"));
    }

    #[test]
    fn a_matched_entry_cooldowns_and_others_kill_on() {
        let mut cooldown = HashMap::new();
        let target = |pid, entry: &str| Target {
            pid,
            create: 1,
            name: "x.exe".into(),
            entry: entry.into(),
        };
        let due = cooled(
            vec![target(10, "a.exe"), target(20, "b.exe")],
            &mut cooldown,
        );
        assert_eq!(due.iter().map(|t| t.pid).collect::<Vec<_>>(), [10, 20]);
        // The same entries inside their cooldown wait; a new entry kills on.
        let due = cooled(
            vec![
                target(11, "a.exe"),
                target(21, "b.exe"),
                target(30, "c.exe"),
            ],
            &mut cooldown,
        );
        assert_eq!(due.iter().map(|t| t.pid).collect::<Vec<_>>(), [30]);
    }
}
