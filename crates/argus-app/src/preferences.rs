//! Native interface preferences, stored atomically alongside application settings.
//!
//! `preferences.json` keeps the layout the WebView interface wrote, so an
//! update carries the preferences over: `{"values": {key: value}, "window":
//! placement}`. Other top-level fields are kept. A file holding the values at
//! its top level (earlier native builds) is read as values and rewritten in
//! that layout on the next change.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::Path,
    sync::Mutex,
};
pub type Preferences = BTreeMap<String, Value>;
static WRITE: Mutex<()> = Mutex::new(());

/// The window's normal (restored) bounds in screen pixels and whether it was
/// maximized.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowPlacement {
    pub rect: [i32; 4],
    pub maximized: bool,
}

impl WindowPlacement {
    /// Plausible bounds: negative coordinates are fine (a monitor left of or
    /// above the primary one), corrupt or degenerate sizes are not.
    pub fn valid(self) -> bool {
        let [left, top, right, bottom] = self.rect;
        self.rect.iter().all(|v| v.abs_diff(0) <= 100_000)
            && (100..=32_768).contains(&(i64::from(right) - i64::from(left)))
            && (100..=32_768).contains(&(i64::from(bottom) - i64::from(top)))
    }
}

/// The file's top-level object; a missing file is an empty one.
fn read_root(path: &Path) -> io::Result<Map<String, Value>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(e),
    }
}

/// Whether the top level holds the values themselves (an earlier native build's file).
fn flat(root: &Map<String, Value>) -> bool {
    !root.is_empty() && !root.contains_key("values") && !root.contains_key("window")
}

fn values_of(root: &Map<String, Value>) -> Preferences {
    if flat(root) {
        return root.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    }
    match root.get("values") {
        Some(Value::Object(values)) => values.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        _ => Preferences::new(),
    }
}

pub fn load(path: &Path) -> io::Result<Preferences> {
    read_root(path).map(|root| values_of(&root))
}

/// The saved window placement, if any and plausible.
pub fn window(path: &Path) -> Option<WindowPlacement> {
    let root = read_root(path).ok()?;
    serde_json::from_value::<WindowPlacement>(root.get("window")?.clone())
        .ok()
        .filter(|p| p.valid())
}

fn save(path: &Path, root: &Map<String, Value>) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("preference directory missing"))?;
    fs::create_dir_all(parent)?;
    let temp = path.with_extension("json.tmp");
    {
        let mut file = fs::File::create(&temp)?;
        file.write_all(&serde_json::to_vec_pretty(root)?)?;
        file.sync_all()?;
    }
    fs::rename(temp, path)
}

/// Reads, changes and replaces the file under the write lock. Malformed data
/// is never overwritten.
fn update(
    path: &Path,
    change: impl FnOnce(&mut Preferences, &mut Map<String, Value>),
) -> io::Result<()> {
    let _lock = WRITE.lock().unwrap();
    let mut root = read_root(path)?;
    let mut values = values_of(&root);
    if flat(&root) {
        root.clear();
    }
    change(&mut values, &mut root);
    root.insert("values".into(), Value::Object(values.into_iter().collect()));
    save(path, &root)
}

pub fn set(path: &Path, key: String, value: Value) -> io::Result<()> {
    set_many(path, vec![(key, value)])
}
/// Whether `key` can name a preference.
pub fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= 1024
}
/// Stores several preferences with one atomic replacement, in order: a later
/// entry for the same key wins.
pub fn set_many(path: &Path, entries: Vec<(String, Value)>) -> io::Result<()> {
    if entries.iter().any(|(key, _)| !valid_key(key)) {
        return Err(io::Error::other("invalid preference key"));
    }
    update(path, |values, _| values.extend(entries))
}
/// Saves the window placement, keeping the values.
pub fn save_window(path: &Path, placement: WindowPlacement) -> io::Result<()> {
    if !placement.valid() || window(path) == Some(placement) {
        return Ok(());
    }
    update(path, |_, root| {
        root.insert(
            "window".into(),
            serde_json::to_value(placement).unwrap_or(Value::Null),
        );
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("argus-pref-{}-{name}.json", std::process::id()))
    }
    #[test]
    fn updates_preserve_other_preferences() {
        let file = path("preserve");
        let _ = fs::remove_file(&file);
        set(&file, "navPosition".into(), "side".into()).unwrap();
        set(
            &file,
            "rowMarks.services".into(),
            serde_json::json!({"service":{"note":"note","pinned":true}}),
        )
        .unwrap();
        assert_eq!(load(&file).unwrap()["navPosition"], "side");
        fs::remove_file(file).unwrap();
    }
    #[test]
    fn malformed_existing_data_is_never_overwritten() {
        let file = path("broken");
        fs::write(&file, b"{broken").unwrap();
        assert!(set(&file, "view".into(), "list".into()).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"{broken");
        fs::remove_file(file).unwrap();
    }
    #[test]
    fn the_webview_interface_layout_is_read_and_kept() {
        let file = path("webview");
        fs::write(
            &file,
            br#"{"values":{"sort":{"key":"mem","desc":false}},"window":{"rect":[-1500,40,-200,850],"maximized":true}}"#,
        )
        .unwrap();
        assert_eq!(load(&file).unwrap()["sort"]["key"], "mem");
        set(&file, "page".into(), "services".into()).unwrap();
        let rect = WindowPlacement {
            rect: [-1500, 40, -200, 850],
            maximized: true,
        };
        assert_eq!(window(&file), Some(rect));
        assert_eq!(load(&file).unwrap()["page"], "services");
        let moved = WindowPlacement {
            rect: [10, 20, 900, 700],
            maximized: false,
        };
        save_window(&file, moved).unwrap();
        assert_eq!(window(&file), Some(moved));
        assert_eq!(load(&file).unwrap()["sort"]["key"], "mem");
        fs::remove_file(file).unwrap();
    }
    #[test]
    fn a_flat_native_file_becomes_values() {
        let file = path("flat");
        fs::write(&file, br#"{"view":"tree","detailsWidth":400}"#).unwrap();
        assert_eq!(load(&file).unwrap()["view"], "tree");
        set(&file, "page".into(), "network".into()).unwrap();
        let root: Map<String, Value> = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        assert_eq!(root["values"]["view"], "tree");
        assert_eq!(root["values"]["page"], "network");
        assert!(!root.contains_key("view"));
        fs::remove_file(file).unwrap();
    }
    #[test]
    fn geometry_rejects_corruption_and_preserves_negative_monitor_coordinates() {
        let placement = |rect| WindowPlacement {
            rect,
            maximized: false,
        };
        assert!(placement([-1600, 30, -300, 800]).valid());
        assert!(!placement([i32::MIN, 0, i32::MAX, 800]).valid());
        assert!(!placement([0, 0, 0, 800]).valid());
    }
}
