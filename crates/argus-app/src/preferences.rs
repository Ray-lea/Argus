//! Native interface preferences, stored atomically alongside application settings.
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::Path,
    sync::Mutex,
};
pub type Preferences = BTreeMap<String, Value>;
static WRITE: Mutex<()> = Mutex::new(());
pub fn load(path: &Path) -> io::Result<Preferences> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Preferences::new()),
        Err(e) => Err(e),
    }
}
fn save(path: &Path, values: &Preferences) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("preference directory missing"))?;
    fs::create_dir_all(parent)?;
    let temp = path.with_extension("json.tmp");
    {
        let mut file = fs::File::create(&temp)?;
        file.write_all(&serde_json::to_vec_pretty(values)?)?;
        file.sync_all()?;
    }
    fs::rename(temp, path)
}
pub fn set(path: &Path, key: String, value: Value) -> io::Result<()> {
    if key.is_empty() || key.len() > 1024 {
        return Err(io::Error::other("invalid preference key"));
    }
    let _lock = WRITE.lock().unwrap();
    let mut current = load(path)?;
    current.insert(key, value);
    save(path, &current)
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
}
