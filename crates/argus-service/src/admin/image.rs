//! Detect an in-place rebuild without rereading executable bytes on every poll.
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};

#[derive(PartialEq, Eq)]
struct Stamp {
    path: PathBuf,
    length: u64,
    modified: SystemTime,
}
impl Stamp {
    fn read(path: &Path) -> std::io::Result<Self> {
        let metadata = path.metadata()?;
        Ok(Self {
            path: path.into(),
            length: metadata.len(),
            modified: metadata.modified()?,
        })
    }
}
struct Comparison {
    image: Stamp,
    source: Stamp,
    different: bool,
}
#[derive(Default)]
struct Cache(Option<Comparison>);
impl Cache {
    fn changed(&mut self, image: &Path, source: &Path) -> std::io::Result<bool> {
        let image = Stamp::read(image)?;
        let source = Stamp::read(source)?;
        if let Some(cached) = &self.0
            && cached.image == image
            && cached.source == source
        {
            return Ok(cached.different);
        }
        let different =
            image.length != source.length || digest(&image.path)? != digest(&source.path)?;
        self.0 = Some(Comparison {
            image,
            source,
            different,
        });
        Ok(different)
    }
}
fn digest(path: &Path) -> std::io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 65536];
    loop {
        let count = file.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
    }
    Ok(hash.finalize().into())
}
pub(super) fn changed(image: &Path, source: &Path) -> bool {
    static CACHE: Mutex<Cache> = Mutex::new(Cache(None));
    // A missing portable source leaves the already authorized copy usable.
    CACHE
        .lock()
        .unwrap()
        .changed(image, source)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn equal_version_source_changes_invalidate_the_content_cache() {
        let root = std::env::temp_dir().join(format!(
            "argus-build-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let image = root.join("authorized.exe");
        let source = root.join("source.exe");
        std::fs::write(&image, b"first build").unwrap();
        std::fs::write(&source, b"first build").unwrap();
        let mut cache = Cache::default();
        assert!(!cache.changed(&image, &source).unwrap());
        assert!(!cache.changed(&image, &source).unwrap());
        std::fs::write(&source, b"next build!").unwrap();
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&source)
            .unwrap();
        file.set_modified(SystemTime::now() + std::time::Duration::from_secs(1))
            .unwrap();
        drop(file);
        assert!(cache.changed(&image, &source).unwrap());
        std::fs::write(&image, b"next build!").unwrap();
        assert!(!cache.changed(&image, &source).unwrap());
        std::fs::remove_file(&source).unwrap();
        assert!(cache.changed(&image, &source).is_err());
        std::fs::remove_file(image).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
