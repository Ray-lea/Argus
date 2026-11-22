//! The folder of an installation outside Program Files.
//!
//! Elevated code runs what that folder holds: the installer's and the
//! uninstaller's maintenance steps, and Argus itself when an administrator
//! lets it install the privileged service. Only administrators can change
//! Program Files. Another folder is as hard to change only on a local volume
//! that keeps permissions, below folders that other accounts can neither
//! rename nor re-permit, and owned by Administrators with a protected DACL
//! like the one Program Files hands down. `assess` says what stands in the
//! way; `prepare` creates the folder that way, or replaces one that holds only
//! Argus's own files. A folder with other files keeps its permissions, and
//! nothing above the folder is changed.

use std::path::{Component, Path, PathBuf, Prefix};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
use windows::Win32::Security::Authorization::{SE_FILE_OBJECT, SetSecurityInfo};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSID,
};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    GetDriveTypeW, GetFileInformationByHandle, GetVolumeInformationW, MOVE_FILE_FLAGS, MoveFileExW,
    OPEN_EXISTING, QueryDosDeviceW, READ_CONTROL, RemoveDirectoryW, WRITE_DAC, WRITE_OWNER,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::SystemServices::FILE_PERSISTENT_ACLS;
use windows::Win32::System::WindowsProgramming::DRIVE_FIXED;
use windows::Win32::UI::Shell::{
    FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
};
use windows::core::{GUID, PCWSTR};

use crate::security::{self, Hold, PathError, PathInspection};
use crate::util::{Handle, UNSAFE_PATH, wide, win};

/// What an installation keeps in its folder. A folder that holds only these,
/// or nothing, can be replaced by a new one.
const OURS: [&str; 2] = ["argus.exe", "uninstall.exe"];

/// More entries than an installation folder ever holds: not looked through.
const MAX_ENTRIES: usize = 256;

/// Where a folder is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// In Program Files, which only administrators can change. Left as it is.
    ProgramFiles,
    /// Somewhere permissions cannot protect: not this computer's own fixed
    /// disk (removable, network, SUBST), a file system without permissions,
    /// or a path that Win32 would rewrite. Says which, for the log.
    Unsupported(&'static str),
    /// On a local volume that keeps permissions.
    Local,
}

/// What the folder itself is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contents {
    /// Not there: `prepare` creates it, and the folders above that are missing.
    Missing,
    /// Owned by SYSTEM, Administrators or TrustedInstaller, and neither it nor
    /// anything in it is writable by another account.
    Secure,
    /// Not secure, and holding only `OURS`, or nothing: `prepare` replaces it.
    Ours,
    /// Not secure, with other files or folders, or a link: left as it is.
    Shared,
}

/// The first folder above that another account can rename or re-permit.
#[derive(Debug, Clone)]
pub struct Exposed {
    pub path: PathBuf,
    /// Which check failed, in fixed words, for the log.
    pub detail: String,
}

pub struct Assessment {
    pub place: Place,
    pub parent: Option<Exposed>,
    pub contents: Contents,
    /// Why the folder is not `Secure`, in fixed words, for the log.
    pub detail: Option<String>,
    /// Pins the folders above against renaming while the assessment lives.
    _pins: Vec<Handle>,
}

impl Assessment {
    fn new(place: Place) -> Self {
        Self {
            place,
            parent: None,
            contents: Contents::Missing,
            detail: None,
            _pins: Vec::new(),
        }
    }

    /// Whether, after `prepare`, only administrators can change the folder
    /// and what it holds.
    pub fn protected(&self) -> bool {
        match self.place {
            Place::ProgramFiles => true,
            Place::Unsupported(_) => false,
            Place::Local => self.parent.is_none() && self.contents != Contents::Shared,
        }
    }

    /// Whether a new folder directly below the volume root would be protected.
    pub fn root_protected(&self) -> bool {
        self.place == Place::Local
            && self
                .parent
                .as_ref()
                .is_none_or(|exposed| exposed.path.parent().is_some())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Failure {
    pub stage: &'static str,
    pub code: u32,
}

/// The default location's parent, as the installer names it.
pub fn program_files() -> Option<PathBuf> {
    known_folder(&FOLDERID_ProgramFiles)
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    let raw = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.ok()?;
    let path = unsafe { raw.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    path.map(PathBuf::from)
}

/// Whether `path` is `base` or below it, ignoring case as NTFS does.
fn within(path: &Path, base: &Path) -> bool {
    let path: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().collect();
    let base = base.as_os_str().to_string_lossy();
    let base: Vec<u16> = base.trim_end_matches('\\').encode_utf16().collect();
    path.len() >= base.len()
        && !base.is_empty()
        && unsafe { CompareStringOrdinal(&path[..base.len()], &base, true) } == CSTR_EQUAL
        && path.get(base.len()).is_none_or(|&c| c == u16::from(b'\\'))
}

/// The drive letter and the folders from the volume root down to `dir`.
/// Only `X:\name\…` paths whose names Win32 keeps as they are.
fn components(dir: &Path) -> std::result::Result<(u8, Vec<PathBuf>), &'static str> {
    let mut parts = dir.components();
    let letter = match parts.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(letter) if letter.is_ascii_alphabetic() => letter.to_ascii_uppercase(),
            _ => return Err("not a drive letter path"),
        },
        _ => return Err("not a drive letter path"),
    };
    if parts.next() != Some(Component::RootDir) {
        return Err("relative path");
    }
    let mut path = PathBuf::from(format!("{}:\\", letter as char));
    let mut list = vec![path.clone()];
    for part in parts {
        let Component::Normal(name) = part else {
            return Err("relative component");
        };
        let name = name.to_string_lossy();
        if name.is_empty()
            || name.ends_with(['.', ' '])
            || name.contains([':', '\u{fffd}'])
            || reserved(&name)
        {
            return Err("name Win32 rewrites");
        }
        path.push(&*name);
        list.push(path.clone());
    }
    if list.len() < 2 {
        return Err("volume root");
    }
    Ok((letter, list))
}

/// A device name (`NUL`, `COM1.txt`), which Win32 opens instead of a folder.
fn reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ((upper.starts_with("COM") || upper.starts_with("LPT"))
        && upper[3..].chars().count() == 1
        && upper[3..]
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '¹' | '²' | '³')))
}

/// Why the drive cannot protect a folder, if it cannot.
fn volume(letter: u8) -> Option<&'static str> {
    let root = wide(&format!("{}:\\", letter as char));
    if unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) } != DRIVE_FIXED {
        return Some("not a fixed drive");
    }
    // SUBST maps a letter to a folder (\??\C:\…) in this logon session only.
    let mut target = [0u16; 512];
    let device = wide(&format!("{}:", letter as char));
    let length = unsafe { QueryDosDeviceW(PCWSTR(device.as_ptr()), Some(&mut target)) } as usize;
    let end = target[..length.min(target.len())]
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(0);
    if !String::from_utf16_lossy(&target[..end]).starts_with(r"\Device\") {
        return Some("drive letter is not a device");
    }
    let mut flags = 0u32;
    if unsafe {
        GetVolumeInformationW(
            PCWSTR(root.as_ptr()),
            None,
            None,
            None,
            Some(&mut flags),
            None,
        )
    }
    .is_err()
    {
        return Some("volume information unavailable");
    }
    if flags & FILE_PERSISTENT_ACLS == 0 {
        return Some("file system without permissions");
    }
    None
}

fn describe(error: PathError, trace: &PathInspection) -> String {
    format!(
        "{}; check={trace:?}",
        argus_core::diagnostics::code(error.code())
    )
}

/// Looks at `dir` and the folders above it. Opens and reads; changes nothing.
pub fn assess(dir: &Path) -> std::result::Result<Assessment, Failure> {
    if [&FOLDERID_ProgramFiles, &FOLDERID_ProgramFilesX86]
        .into_iter()
        .filter_map(known_folder)
        .any(|base| within(dir, &base))
    {
        return Ok(Assessment::new(Place::ProgramFiles));
    }
    let (letter, list) = match components(dir) {
        Ok(found) => found,
        Err(reason) => return Ok(Assessment::new(Place::Unsupported(reason))),
    };
    if let Some(reason) = volume(letter) {
        return Ok(Assessment::new(Place::Unsupported(reason)));
    }
    let mut assessment = Assessment::new(Place::Local);
    let (dir, above) = list.split_last().expect("a folder below the root");
    for (index, path) in above.iter().enumerate() {
        let hold = if index == 0 { Hold::Root } else { Hold::Parent };
        let mut trace = PathInspection::default();
        match security::inspect_held(path, hold, &mut trace) {
            Ok(pin) => assessment._pins.push(pin),
            // The rest is created by `prepare`, with the folder.
            Err(PathError::Os(2 | 3)) if index > 0 => return Ok(assessment),
            Err(error) => {
                assessment.parent = Some(Exposed {
                    path: path.clone(),
                    detail: describe(error, &trace),
                });
                break;
            }
        }
    }
    let (contents, detail) = contents(dir);
    assessment.contents = contents;
    assessment.detail = detail;
    Ok(assessment)
}

fn contents(dir: &Path) -> (Contents, Option<String>) {
    let mut trace = PathInspection::default();
    // Why it is not secure, once something is found.
    let mut exposed = match security::inspect_held(dir, Hold::Content, &mut trace) {
        Err(PathError::Os(2 | 3)) => return (Contents::Missing, None),
        Err(error @ PathError::ReparsePoint) => {
            return (Contents::Shared, Some(describe(error, &trace)));
        }
        Ok(_) => None,
        Err(error) => Some(describe(error, &trace)),
    };
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            let code = error.raw_os_error().unwrap_or(1) as u32;
            return (
                Contents::Shared,
                Some(format!(
                    "stage=list folder; {}",
                    argus_core::diagnostics::code(code)
                )),
            );
        }
    };
    let mut ours = true;
    for (index, entry) in entries.enumerate() {
        let Ok(entry) = entry else {
            return (Contents::Shared, Some("stage=list folder entry".into()));
        };
        if index >= MAX_ENTRIES {
            return (
                Contents::Shared,
                Some("stage=list folder; too many entries".into()),
            );
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let file = entry.file_type().is_ok_and(|kind| kind.is_file());
        ours &= file && OURS.iter().any(|known| known.eq_ignore_ascii_case(&name));
        if exposed.is_none() {
            let mut trace = PathInspection::default();
            if let Err(error) = security::inspect_held(&entry.path(), Hold::Content, &mut trace) {
                exposed = Some(format!("entry {}", describe(error, &trace)));
            }
        }
    }
    match exposed {
        None => (Contents::Secure, None),
        Some(detail) if ours => (Contents::Ours, Some(detail)),
        Some(detail) => (Contents::Shared, Some(detail)),
    }
}

/// Before the installer writes to `dir`: creates it as only administrators
/// may change it, or replaces it when it holds only Argus's files. A folder
/// in Program Files or where permissions cannot protect it is created as
/// before, a folder with other files is left as it is. Fails only when the
/// folder cannot be made at all; what cannot be protected is in `log`.
pub fn prepare(
    dir: &Path,
    log: &mut dyn FnMut(String),
) -> std::result::Result<Assessment, Failure> {
    let assessment = assess(dir)?;
    match (assessment.place, assessment.contents) {
        (Place::ProgramFiles | Place::Unsupported(_), _) => {
            std::fs::create_dir_all(dir).map_err(|error| Failure {
                stage: "create install folder",
                code: error.raw_os_error().unwrap_or(1) as u32,
            })?;
        }
        (Place::Local, Contents::Secure | Contents::Shared) => {}
        (Place::Local, Contents::Missing) => create(dir, assessment.parent.is_none())?,
        (Place::Local, Contents::Ours) => replace(dir, log)?,
    }
    Ok(assessment)
}

fn create_protected(path: &Path) -> std::result::Result<(), u32> {
    let sd = security::installation_descriptor()?;
    unsafe {
        CreateDirectoryW(
            PCWSTR(wide(&path.to_string_lossy()).as_ptr()),
            Some(&sd.attributes()),
        )
    }
    .map_err(win)
}

/// Creates the missing folders down to `dir`, each as only administrators
/// may change it. The folder itself appearing meanwhile, or (when `checked`,
/// the folders above having passed) a folder above that others can change,
/// is someone else's doing: stop.
fn create(dir: &Path, checked: bool) -> std::result::Result<(), Failure> {
    let (_, list) = components(dir).map_err(|_| Failure {
        stage: "install folder path",
        code: 87,
    })?;
    let mut pins = Vec::new();
    for path in &list[1..] {
        if path != dir && std::fs::symlink_metadata(path).is_ok() {
            if checked {
                let mut trace = PathInspection::default();
                pins.push(
                    security::inspect_held(path, Hold::Parent, &mut trace).map_err(|error| {
                        Failure {
                            stage: "install folder changed",
                            code: error.code(),
                        }
                    })?,
                );
            }
            continue;
        }
        create_protected(path).map_err(|code| Failure {
            stage: "create protected install folder",
            code,
        })?;
    }
    Ok(())
}

/// A folder others could change, holding only an earlier installation of
/// Argus (or nothing). Moves it aside and makes a new one in its place, so
/// handles opened on the old folder or its files keep no hold on the new;
/// then deletes the old files, which the installer writes anew. When the old
/// folder cannot be moved (a program has it open), sets the new permissions
/// on it instead.
fn replace(dir: &Path, log: &mut dyn FnMut(String)) -> std::result::Result<(), Failure> {
    let name = dir.file_name().ok_or(Failure {
        stage: "install folder path",
        code: 87,
    })?;
    let id = crate::taskmgr::id().map_err(|_| Failure {
        stage: "CoCreateGuid",
        code: 1,
    })?;
    let aside = dir.with_file_name(format!("{}.old-{}", name.to_string_lossy(), &id[..8]));
    let from = wide(&dir.to_string_lossy());
    let to = wide(&aside.to_string_lossy());
    if let Err(error) = unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVE_FILE_FLAGS(0),
        )
    } {
        log(format!(
            "install folder not moved aside; {}; setting permissions in place",
            argus_core::diagnostics::code(win(error))
        ));
        in_place(dir, log);
        return Ok(());
    }
    if let Err(code) = create_protected(dir) {
        let back = unsafe {
            MoveFileExW(
                PCWSTR(to.as_ptr()),
                PCWSTR(from.as_ptr()),
                MOVE_FILE_FLAGS(0),
            )
        };
        log(format!(
            "previous folder {} restored: {}",
            argus_core::diagnostics::path(&aside),
            back.is_ok()
        ));
        return Err(Failure {
            stage: "create protected install folder",
            code,
        });
    }
    remove_ours(&aside, log);
    if let Err(error) = unsafe { RemoveDirectoryW(PCWSTR(to.as_ptr())) } {
        log(format!(
            "previous folder kept; {}; path={}",
            argus_core::diagnostics::code(win(error)),
            argus_core::diagnostics::path(&aside)
        ));
    }
    Ok(())
}

fn remove_ours(dir: &Path, log: &mut dyn FnMut(String)) {
    for name in OURS {
        let path = dir.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => log(format!(
                "previous file kept; {}; path={}",
                argus_core::diagnostics::io(&error),
                argus_core::diagnostics::path(&path)
            )),
        }
    }
}

/// Gives `dir` the owner and protected DACL `create_protected` would, then
/// deletes the old files so the installer creates them under it. What fails
/// is logged; the installation goes on as before this check existed.
fn in_place(dir: &Path, log: &mut dyn FnMut(String)) {
    let result = (|| {
        let path = wide(&dir.to_string_lossy());
        let handle = Handle(
            unsafe {
                CreateFileW(
                    PCWSTR(path.as_ptr()),
                    (READ_CONTROL | WRITE_DAC | WRITE_OWNER).0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                    None,
                )
            }
            .map_err(|error| ("open install folder for permissions", win(error)))?,
        );
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(handle.0, &mut info) }
            .map_err(|error| ("GetFileInformationByHandle", win(error)))?;
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err(("install folder is a link", UNSAFE_PATH));
        }
        set_protected(handle.0).map_err(|code| ("SetSecurityInfo", code))
    })();
    match result {
        Ok(()) => remove_ours(dir, log),
        Err((stage, code)) => log(format!(
            "install folder permissions unchanged; stage={stage}; {}",
            argus_core::diagnostics::code(code)
        )),
    }
}

fn set_protected(handle: HANDLE) -> std::result::Result<(), u32> {
    let sd = security::installation_descriptor()?;
    let mut owner = PSID::default();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut present = false.into();
    let mut defaulted = false.into();
    unsafe { GetSecurityDescriptorOwner(sd.0, &mut owner, &mut defaulted) }.map_err(win)?;
    unsafe { GetSecurityDescriptorDacl(sd.0, &mut present, &mut dacl, &mut defaulted) }
        .map_err(win)?;
    // Inheritable entries reach what the folder holds; the old files go.
    let status = unsafe {
        SetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION,
            Some(owner),
            None,
            Some(dacl),
            None,
        )
    };
    if status.0 == 0 { Ok(()) } else { Err(status.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("argus-folder-{}", crate::taskmgr::id().unwrap()));
        std::fs::create_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn only_plain_drive_letter_paths_are_protected() {
        let (letter, list) = components(Path::new(r"d:\Tools\Argus")).unwrap();
        assert_eq!(letter, b'D');
        assert_eq!(
            list,
            [r"D:\", r"D:\Tools", r"D:\Tools\Argus"].map(PathBuf::from)
        );
        for path in [
            r"D:\",
            r"D:Argus",
            r"\\server\share\Argus",
            r"\\?\D:\Argus",
            r"D:\Tools\..\Argus",
            r"D:\Argus.",
            r"D:\Argus ",
            r"D:\Argus:stream",
            r"D:\NUL\Argus",
            r"D:\com1.txt",
            r"D:\LPT¹",
            r"Argus",
        ] {
            assert!(components(Path::new(path)).is_err(), "{path}");
        }
        assert!(components(Path::new(r"D:\COMX\Console\Argus")).is_ok());
    }

    #[test]
    fn program_files_matches_whole_names_ignoring_case() {
        let base = Path::new(r"C:\Program Files");
        assert!(within(Path::new(r"c:\program files\Argus"), base));
        assert!(within(Path::new(r"C:\Program Files"), base));
        assert!(within(
            Path::new(r"C:\Program Files\Argus"),
            Path::new(r"C:\Program Files\")
        ));
        assert!(!within(Path::new(r"C:\Program Files (x86)\Argus"), base));
        assert!(!within(Path::new(r"C:\Program"), base));
        assert!(!within(Path::new(r"D:\Program Files\Argus"), base));
    }

    #[test]
    fn a_folder_of_argus_files_is_replaced_and_one_with_others_kept() {
        // A test's own folders belong to its account, so none is secure.
        let dir = scratch();
        assert_eq!(contents(&dir).0, Contents::Ours);
        std::fs::write(dir.join("argus.exe"), b"").unwrap();
        std::fs::write(dir.join("Uninstall.exe"), b"").unwrap();
        let (found, detail) = contents(&dir);
        assert_eq!(found, Contents::Ours);
        assert!(detail.is_some());
        std::fs::write(dir.join("version.dll"), b"").unwrap();
        assert_eq!(contents(&dir).0, Contents::Shared);
        std::fs::remove_file(dir.join("version.dll")).unwrap();
        std::fs::create_dir(dir.join("argus.exe.local")).unwrap();
        assert_eq!(contents(&dir).0, Contents::Shared);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(contents(&dir).0, Contents::Missing);
    }

    #[test]
    fn a_folder_below_an_account_profile_is_exposed() {
        // Temp lies in the account's own profile (or the CI runner's work
        // folder): the account can rename what is above the new folder.
        let dir = scratch();
        let target = dir.join("Argus");
        let assessment = assess(&target).unwrap();
        if assessment.place == Place::Local {
            let exposed = assessment.parent.as_ref().expect("an exposed folder");
            assert!(target.starts_with(&exposed.path));
            assert!(!assessment.protected());
            assert_eq!(assessment.contents, Contents::Missing);
        }
        drop(assessment);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
