//! The process blacklist: names the user never wants running.
//!
//! The details pane adds a selected process's image name; the blacklist page
//! lists the names and removes them. While the list is not empty, every
//! sample is swept off the UI thread: any process whose image name matches an
//! entry is terminated at once, so bundleware and its helpers die as they
//! appear. Names match exactly (case-insensitive); `notepad` reads as
//! `notepad.exe`.
use super::model::{text, values};
use super::prefs::Prefs;
use serde_json::Value;

/// The preference key the names live under.
pub(in crate::frontend) const KEY: &str = "processBlacklist";

/// One blacklisted name, as stored: trimmed and lowercased, always with an
/// extension so `notepad` and `notepad.exe` are the same entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::frontend) struct Name(pub String);

impl Name {
    fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        let name = raw.to_lowercase();
        Some(Self(if raw.contains('.') {
            name
        } else {
            format!("{name}.exe")
        }))
    }
}

/// The blacklisted names, in the order added.
pub(in crate::frontend) fn names(prefs: &Prefs) -> Vec<String> {
    prefs
        .get::<Vec<String>>(KEY, vec![])
        .iter()
        .filter_map(|name| Name::parse(name).map(|n| n.0))
        .collect()
}

/// Adds one name; returns the stored form, or `None` for blank text or a
/// name already present.
pub(in crate::frontend) fn add(prefs: &mut Prefs, raw: &str) -> Option<String> {
    let name = Name::parse(raw)?;
    let mut list = names(prefs);
    if list.iter().any(|stored| *stored == name.0) {
        return None;
    }
    list.push(name.0.clone());
    prefs.set(KEY, &list);
    Some(name.0)
}

/// Removes one name, in either form (`notepad` removes `notepad.exe`).
pub(in crate::frontend) fn remove(prefs: &mut Prefs, raw: &str) {
    let Some(name) = Name::parse(raw) else {
        return;
    };
    let list = names(prefs);
    let kept: Vec<String> = list
        .into_iter()
        .filter(|stored| *stored != name.0)
        .collect();
    prefs.set(KEY, &kept);
}

/// Whether an image name is blacklisted.
pub(in crate::frontend) fn holds(prefs: &Prefs, image: &str) -> bool {
    let image = Name::parse(image);
    image.is_some_and(|image| names(prefs).iter().any(|stored| *stored == image.0))
}

/// The processes of a sample the blacklist asks to terminate: their PID,
/// creation time and image name. Pending identities (a creation time of
/// less than 0) wait for a later sample, as the end action does.
pub(in crate::frontend) fn targets(
    snapshot: &Value,
    prefs: &Prefs,
    own_image: &str,
) -> Vec<(u32, i64, String)> {
    let own = Name::parse(own_image);
    values(&snapshot["processes"])
        .iter()
        .filter_map(|p| {
            let pid = p["pid"].as_u64()?;
            let create = p["createTime"].as_f64()?;
            if create < 0. {
                return None;
            }
            let (pid, create) = (u32::try_from(pid).ok()?, create as i64);
            let image = text(p, "name");
            // Argus never blacklists itself, whatever the list holds.
            if own
                .as_ref()
                .is_some_and(|own| *own.0 == image.to_lowercase())
            {
                return None;
            }
            holds(prefs, &image).then(|| (pid, create, image.to_owned()))
        })
        .collect()
}

/// Sweeps one sample: terminates every blacklisted process off the UI thread
/// and refreshes the list, the way an end action does. A fixture never
/// reaches real processes, so its samples are left alone.
pub(in crate::frontend) fn sweep(
    host: &crate::native_runtime::Host,
    targets: &[(u32, i64, String)],
) {
    if targets.is_empty() {
        return;
    }
    let host = host.clone();
    let targets = targets.to_vec();
    std::thread::spawn(move || {
        for (pid, create, image) in targets {
            match crate::dispatch::invoke(
                &host,
                "kill_process",
                serde_json::json!({"pid":pid,"createTime":create,"force":false}),
            ) {
                Ok(_) => log::info!("blacklist: terminated {image} (pid {pid})"),
                Err(error) => log::warn!("blacklist: {image} (pid {pid}) survived: {error}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    // Not the glob: the frontend's imports bring GPUI's own `test` attribute.
    use super::{Name, add, holds, names, remove, targets};
    use crate::frontend::prefs::Prefs;
    use serde_json::json;

    fn store() -> Prefs {
        Prefs::load(
            std::env::temp_dir().join(format!("argus-blacklist-test-{}.json", std::process::id())),
        )
    }

    #[test]
    fn names_normalize_and_extensions_fall_back() {
        assert_eq!(Name::parse(" 360SE.EXE ").unwrap().0, "360se.exe");
        assert_eq!(Name::parse("Notepad").unwrap().0, "notepad.exe");
        assert_eq!(Name::parse("  "), None);
        assert!(Name::parse("").is_none());
    }

    #[test]
    fn add_is_case_insensitive_and_deduplicates() {
        let mut prefs = store();
        assert_eq!(add(&mut prefs, "wxwork.exe").as_deref(), Some("wxwork.exe"));
        assert_eq!(add(&mut prefs, " QQ ").as_deref(), Some("qq.exe"));
        assert_eq!(add(&mut prefs, "WXWORK.EXE"), None);
        assert_eq!(names(&prefs), ["wxwork.exe", "qq.exe"]);
        // Short form removes the stored extension form.
        remove(&mut prefs, "QQ");
        assert_eq!(names(&prefs), ["wxwork.exe"]);
        assert!(!holds(&prefs, "qq.exe"));
    }

    #[test]
    fn targets_match_image_names_and_spare_argus_and_pending_ids() {
        let mut prefs = store();
        add(&mut prefs, "360se.exe");
        let snapshot = json!({"processes":[
            {"pid":10,"createTime":1.,"name":"360SE.EXE"},
            {"pid":20,"createTime":2.,"name":"chrome.exe"},
            {"pid":30,"createTime":-1.,"name":"360se.exe"},
            {"pid":40,"createTime":4.,"name":"explorer.exe"}
        ]});
        let own = "argus.exe";
        assert_eq!(
            targets(&snapshot, &prefs, own),
            vec![(10, 1, "360SE.EXE".into())]
        );
        // Without the extension the entry still matched through the fallback.
        add(&mut prefs, "explorer");
        assert_eq!(
            targets(&snapshot, &prefs, own),
            vec![(10, 1, "360SE.EXE".into()), (40, 4, "explorer.exe".into())]
        );
    }
}
