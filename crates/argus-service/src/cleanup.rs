//! Deletion-only access to retired runtime files. These handles confer no trust
//! on file contents and must never be used to authorize or launch a program.
use std::fs;
use std::path::{Component, Path, PathBuf, Prefix};

use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, GetFileInformationByHandle,
    OPEN_EXISTING,
};
use windows::core::PCWSTR;

use crate::util::{Handle, Result, UNSAFE_PATH, io, wide, win};

pub(crate) struct Directory {
    path: PathBuf,
    pins: Vec<Handle>,
}

impl Directory {
    pub(crate) fn open(path: &Path) -> Result<Option<Self>> {
        if !path.is_absolute()
            || !matches!(path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
            || path
                .components()
                .any(|p| matches!(p, Component::CurDir | Component::ParentDir))
        {
            return Err(87);
        }
        let mut pins = Vec::new();
        for part in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
            // Pin every ancestor, including the volume root. Denying write and
            // delete sharing also prevents turning a pinned directory into a
            // junction between validation and a child-file deletion.
            let handle = match unsafe {
                CreateFileW(
                    PCWSTR(wide(&part.to_string_lossy()).as_ptr()),
                    FILE_LIST_DIRECTORY.0 | FILE_READ_ATTRIBUTES.0,
                    FILE_SHARE_READ,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                    None,
                )
            }
            .map_err(win)
            {
                Ok(handle) => Handle(handle),
                Err(2 | 3) => return Ok(None),
                Err(code) => return Err(code),
            };
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            unsafe { GetFileInformationByHandle(handle.0, &mut info) }.map_err(win)?;
            if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
                || info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0
            {
                return Err(UNSAFE_PATH);
            }
            pins.push(handle);
        }
        Ok(Some(Self {
            path: path.into(),
            pins,
        }))
    }

    pub(crate) fn remove_file(&self, name: &str) -> Result<()> {
        let mut parts = Path::new(name).components();
        if name.contains([':', '/', '\\', '\0'])
            || !matches!(parts.next(), Some(Component::Normal(_)))
            || parts.next().is_some()
        {
            return Err(87);
        }
        // DeleteFile removes a leaf symlink itself. Never open or truncate its
        // target, and never recurse into a directory with a matching filename.
        match fs::remove_file(self.path.join(name)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(io(error)),
            _ => Ok(()),
        }
    }

    pub(crate) fn remove_empty(mut self) -> Result<()> {
        self.pins.pop();
        // All ancestors remain pinned until this call finishes.
        match fs::remove_dir(&self.path) {
            Err(error) if !matches!(error.raw_os_error(), Some(2 | 3 | 145)) => Err(io(error)),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Directory;
    use std::fs;
    use std::os::windows::fs::OpenOptionsExt;

    #[test]
    fn user_owned_directory_can_be_cleaned_without_trusting_or_rewriting_it() {
        let root =
            std::env::temp_dir().join(format!("argus-cleanup-{}", crate::taskmgr::id().unwrap()));
        let dir = root.join("runtime");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&dir).unwrap();
        let source = root.join("unrelated.txt");
        fs::write(&source, b"unchanged").unwrap();
        fs::hard_link(&source, dir.join("grant.json")).unwrap();
        fs::write(dir.join("keep.txt"), b"unrelated").unwrap();
        assert!(crate::security::protect_directory(&dir).is_err());
        let pinned = Directory::open(&dir).unwrap().unwrap();
        assert!(fs::rename(&root, root.with_extension("moved")).is_err());
        // Changing reparse data requires a write handle, which the pin denies.
        let writer = fs::OpenOptions::new()
            .access_mode(0x4000_0000)
            .custom_flags(0x0200_0000)
            .open(&dir);
        assert!(writer.is_err());
        assert_eq!(pinned.remove_file("../unrelated.txt"), Err(87));
        pinned.remove_file("grant.json").unwrap();
        pinned.remove_file("grant.json").unwrap();
        assert_eq!(fs::read(&source).unwrap(), b"unchanged");
        pinned.remove_empty().unwrap();
        assert_eq!(fs::read(dir.join("keep.txt")).unwrap(), b"unrelated");
        fs::remove_file(dir.join("keep.txt")).unwrap();
        Directory::open(&dir)
            .unwrap()
            .unwrap()
            .remove_empty()
            .unwrap();
        assert!(Directory::open(&dir).unwrap().is_none());
        fs::remove_file(source).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn directory_junctions_are_rejected_at_the_leaf_and_in_ancestors() {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::IO::DeviceIoControl;

        let root =
            std::env::temp_dir().join(format!("argus-cleanup-{}", crate::taskmgr::id().unwrap()));
        fs::create_dir(&root).unwrap();
        let target = root.join("target");
        let junction = root.join("junction");
        fs::create_dir(&target).unwrap();
        fs::create_dir(&junction).unwrap();
        fs::write(target.join("grant.json"), b"keep").unwrap();
        // A mount-point junction needs no symbolic-link privilege.
        let substitute: Vec<u16> = format!(r"\??\{}", target.display())
            .encode_utf16()
            .collect();
        let length = (substitute.len() * 2) as u16;
        let mut data = 0xa000_0003u32.to_le_bytes().to_vec();
        for value in [length + 12, 0, 0, length, length + 2, 0] {
            data.extend_from_slice(&value.to_le_bytes());
        }
        for value in substitute.into_iter().chain([0, 0]) {
            data.extend_from_slice(&value.to_le_bytes());
        }
        let file = fs::OpenOptions::new()
            .access_mode(0x4000_0000)
            .custom_flags(0x0220_0000)
            .open(&junction)
            .unwrap();
        let mut returned = 0;
        unsafe {
            DeviceIoControl(
                HANDLE(file.as_raw_handle()),
                0x0009_00a4,
                Some(data.as_ptr().cast()),
                data.len() as u32,
                None,
                0,
                Some(&mut returned),
                None,
            )
        }
        .unwrap();
        drop(file);
        assert!(matches!(
            Directory::open(&junction),
            Err(crate::util::UNSAFE_PATH)
        ));
        assert!(matches!(
            Directory::open(&junction.join("child")),
            Err(crate::util::UNSAFE_PATH)
        ));
        assert_eq!(fs::read(target.join("grant.json")).unwrap(), b"keep");
        fs::remove_dir(junction).unwrap();
        fs::remove_file(target.join("grant.json")).unwrap();
        fs::remove_dir(target).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
