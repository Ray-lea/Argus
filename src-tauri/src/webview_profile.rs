//! WebView2 de-elevates into the desktop account, which may differ from the host.

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use windows::Win32::Foundation::{GetLastError, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{
    ImpersonateLoggedOnUser, RevertToSelf, TOKEN_DUPLICATE, TOKEN_IMPERSONATE, TOKEN_QUERY,
    TOKEN_USER, TokenSessionId, TokenUser,
};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
    FILE_READ_ATTRIBUTES, FILE_SHARE_READ, GetFileInformationByHandle, GetFinalPathNameByHandleW,
    VOLUME_NAME_DOS,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThreadId, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath};
use windows::Win32::UI::WindowsAndMessaging::{
    GetShellWindow, GetWindowThreadProcessId, MB_ICONERROR, MB_OK, MessageBoxW,
};
use windows::core::{PCWSTR, PWSTR};

use crate::tasks::{Handle, token_buffer, wide};

#[link(name = "user32")]
unsafe extern "system" {
    fn GetThreadDesktop(thread: u32) -> *mut std::ffi::c_void;
    fn GetUserObjectInformationW(
        handle: *mut std::ffi::c_void,
        index: i32,
        info: *mut std::ffi::c_void,
        length: u32,
        needed: *mut u32,
    ) -> i32;
}

#[derive(Debug)]
pub struct ProfileError {
    pub stage: &'static str,
    pub code: u32,
    pub path: Option<PathBuf>,
}
impl ProfileError {
    fn win(stage: &'static str, error: windows::core::Error) -> Self {
        let code = error.code().0 as u32;
        Self {
            stage,
            path: None,
            code: if code & 0xffff0000 == 0x80070000 {
                code & 0xffff
            } else {
                code
            },
        }
    }
    fn io(stage: &'static str, error: std::io::Error) -> Self {
        Self {
            stage,
            path: None,
            code: error.raw_os_error().unwrap_or(31) as u32,
        }
    }

    fn at(mut self, path: &Path) -> Self {
        self.path = Some(path.to_owned());
        self
    }

    pub fn report(&self) {
        let message = wide(&argus_core::tr!(
            "host.webViewProfileFailed",
            code = format!("0x{:08X}", self.code)
        ));
        let title = wide(&crate::shell::app_title());
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(message.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_ICONERROR | MB_OK,
            )
        };
    }
}

pub struct Profile {
    pub directory: PathBuf,
    // Prevent deletion, rename or conversion into a reparse point while the host uses the path.
    _lease: File,
}

struct Impersonation;
impl Drop for Impersonation {
    fn drop(&mut self) {
        if unsafe { RevertToSelf() }.is_err() {
            // Never resume host work under an unexpected thread identity.
            std::process::abort();
        }
    }
}

fn open_token(process: HANDLE, impersonate: bool) -> Result<Handle, ProfileError> {
    let mut token = HANDLE::default();
    let access = TOKEN_QUERY
        | if impersonate {
            TOKEN_DUPLICATE | TOKEN_IMPERSONATE
        } else {
            TOKEN_QUERY
        };
    unsafe { OpenProcessToken(process, access, &mut token) }
        .map_err(|e| ProfileError::win("OpenProcessToken", e))?;
    Ok(Handle(token))
}

fn user(token: HANDLE) -> Result<String, ProfileError> {
    let data = token_buffer(token, TokenUser).map_err(|e| ProfileError {
        path: None,
        stage: "TokenUser",
        code: e.code,
    })?;
    let user = unsafe { &*data.as_ptr().cast::<TOKEN_USER>() };
    let mut sid = PWSTR::null();
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) }
        .map_err(|e| ProfileError::win("ConvertSidToStringSidW", e))?;
    let result = unsafe { sid.to_string() }.map_err(|e| ProfileError::win("SID string", e.into()));
    let _ = unsafe { LocalFree(Some(HLOCAL(sid.0.cast()))) };
    result
}

fn desktop_name() -> Result<String, ProfileError> {
    let desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
    let mut buffer = [0u16; 256];
    let mut needed = 0;
    // UOI_NAME = 2; desktop names are bounded Windows object names.
    if unsafe {
        GetUserObjectInformationW(
            desktop,
            2,
            buffer.as_mut_ptr().cast(),
            size_of_val(&buffer) as u32,
            &mut needed,
        )
    } == 0
    {
        return Err(ProfileError::win(
            "desktop name",
            windows::core::Error::from_thread(),
        ));
    }
    let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    Ok(String::from_utf16_lossy(&buffer[..len]))
}

fn profile_key(host: &str, session: u32, desktop: &str) -> String {
    let digest =
        Sha256::digest(format!("{host}\0{session}\0{}", desktop.to_lowercase()).as_bytes());
    let mut key = String::from("elevated-");
    for byte in &digest[..16] {
        use std::fmt::Write;
        let _ = write!(key, "{byte:02x}");
    }
    key
}

fn known_folder(token: HANDLE) -> Result<PathBuf, ProfileError> {
    let path =
        unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DONT_VERIFY, Some(token)) }
            .map_err(|e| ProfileError::win("desktop LocalAppData", e))?;
    let result = unsafe { path.to_string() }
        .map(PathBuf::from)
        .map_err(|e| ProfileError::win("LocalAppData string", e.into()));
    unsafe { CoTaskMemFree(Some(path.0.cast())) };
    result
}

fn lock_directory(path: &Path) -> Result<(File, PathBuf), ProfileError> {
    lock_directory_inner(path).map_err(|error| error.at(path))
}

fn lock_directory_inner(path: &Path) -> Result<(File, PathBuf), ProfileError> {
    let file = OpenOptions::new()
        .read(true)
        // Attribute-only access does not enforce directory sharing restrictions.
        .access_mode(FILE_READ_ATTRIBUTES.0 | FILE_LIST_DIRECTORY.0)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(|e| ProfileError::io("open browser directory", e))?;
    let handle = HANDLE(file.as_raw_handle());
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(handle, &mut info) }
        .map_err(|e| ProfileError::win("browser directory attributes", e))?;
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(ProfileError {
            path: None,
            stage: "browser directory reparse point",
            code: 4390,
        });
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0 {
        return Err(ProfileError {
            path: None,
            stage: "browser path is not a directory",
            code: 267,
        });
    }
    // The final path resolves ancestor junctions. Keep the lease until the webview closes.
    Ok((file, final_directory(handle)?))
}

fn final_directory(handle: HANDLE) -> Result<PathBuf, ProfileError> {
    let mut buffer = vec![0u16; 32768];
    let len = unsafe { GetFinalPathNameByHandleW(handle, &mut buffer, VOLUME_NAME_DOS) } as usize;
    if len == 0 || len >= buffer.len() {
        return Err(ProfileError {
            path: None,
            stage: "browser directory final path",
            code: if len == 0 {
                unsafe { GetLastError() }.0
            } else {
                122
            },
        });
    }
    let final_path = OsString::from_wide(&buffer[..len]);
    Ok(PathBuf::from(final_path))
}

fn resolve(identifier: &str, create: bool) -> Result<Option<Profile>, ProfileError> {
    let window = unsafe { GetShellWindow() };
    if window.0.is_null() {
        return Ok(None);
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    let process = Handle(
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
            .map_err(|e| ProfileError::win("desktop process", e))?,
    );
    let desktop = open_token(process.0, false)?;
    let own = open_token(unsafe { GetCurrentProcess() }, false)?;
    let desktop_user = user(desktop.0)?;
    let own_user = user(own.0)?;
    if desktop_user == own_user {
        return Ok(None);
    }
    let session = token_buffer(own.0, TokenSessionId).map_err(|e| ProfileError {
        path: None,
        stage: "host session",
        code: e.code,
    })?[0] as u32;
    let desktop_session = token_buffer(desktop.0, TokenSessionId).map_err(|e| ProfileError {
        path: None,
        stage: "desktop session",
        code: e.code,
    })?[0] as u32;
    if session != desktop_session {
        return Err(ProfileError {
            path: None,
            stage: "desktop session mismatch",
            code: 5,
        });
    }
    let key = profile_key(&own_user, session, &desktop_name()?);
    let desktop = open_token(process.0, true)?;
    unsafe { ImpersonateLoggedOnUser(desktop.0) }
        .map_err(|e| ProfileError::win("desktop account impersonation", e))?;
    let _impersonation = Impersonation;
    let directory = known_folder(desktop.0)?
        .join(identifier)
        .join("WebView2")
        .join(key);
    if create {
        // Creation runs with the browser account's rights, including redirected folder paths.
        std::fs::create_dir_all(&directory)
            .map_err(|e| ProfileError::io("create browser directory", e).at(&directory))?;
    } else if !directory.exists() {
        return Ok(None);
    }
    let (lease, directory) = lock_directory(&directory)?;
    Ok(Some(Profile {
        directory,
        _lease: lease,
    }))
}

pub fn prepare(identifier: &str) -> Result<Option<Profile>, ProfileError> {
    worker(identifier, true)
}

/// Maintenance may locate an existing profile, but must not create one during uninstall.
pub fn existing(identifier: &str) -> Result<Option<Profile>, ProfileError> {
    worker(identifier, false)
}

fn worker(identifier: &str, create: bool) -> Result<Option<Profile>, ProfileError> {
    if !argus_core::privilege::is_elevated() {
        return Ok(None);
    }
    let identifier = identifier.to_owned();
    std::thread::Builder::new()
        .name("argus-webview-profile".into())
        .spawn(move || resolve(&identifier, create))
        .map_err(|e| ProfileError::io("browser profile worker", e))?
        .join()
        .unwrap_or(Err(ProfileError {
            path: None,
            stage: "browser profile worker",
            code: 31,
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_path_query_keeps_the_native_failure_instead_of_reporting_not_found() {
        let error = final_directory(windows::Win32::Foundation::INVALID_HANDLE_VALUE).unwrap_err();
        assert_eq!(error.code, 6);
        assert_eq!(error.stage, "browser directory final path");
    }

    #[test]
    fn profile_directory_failures_retain_the_actual_path_before_logging_starts() {
        let path = std::env::temp_dir().join(format!(
            "argus-profile-missing-{:?}",
            unsafe { windows::Win32::System::Com::CoCreateGuid() }.unwrap()
        ));
        let error = lock_directory(&path).err().unwrap();
        assert_eq!(error.path.as_deref(), Some(path.as_path()));
        assert!(matches!(error.code, 2 | 3));
        assert_eq!(error.stage, "open browser directory");
    }
    #[test]
    fn profile_is_isolated_by_host_account_session_and_desktop() {
        let key = profile_key("S-1-5-21-1-2-3-1000", 1, "Default");
        assert_eq!(key, profile_key("S-1-5-21-1-2-3-1000", 1, "default"));
        assert_ne!(key, profile_key("S-1-5-21-1-2-3-1001", 1, "Default"));
        assert_ne!(key, profile_key("S-1-5-21-1-2-3-1000", 2, "Default"));
        assert_ne!(key, profile_key("S-1-5-21-1-2-3-1000", 1, "Private"));
    }
    #[test]
    fn directory_lease_allows_browser_files_but_blocks_replacement() {
        let base = std::env::temp_dir().join(format!("argus-browser-lease-{}", std::process::id()));
        std::fs::create_dir(&base).unwrap();
        let directory = base.join("profile");
        std::fs::create_dir(&directory).unwrap();
        let (lease, resolved) = lock_directory(&directory).unwrap();
        let child = resolved.join("browser.txt");
        std::fs::write(&child, "test").unwrap();
        let replacement = base.join("replacement");
        let renamed = std::fs::rename(&directory, &replacement).is_ok();
        if renamed {
            drop(lease);
            std::fs::remove_file(replacement.join("browser.txt")).unwrap();
            std::fs::remove_dir(replacement).unwrap();
            std::fs::remove_dir(base).unwrap();
            panic!("the leased directory was renamed");
        }
        assert!(std::fs::rename(&base, base.with_extension("moved")).is_err());
        std::fs::remove_file(child).unwrap();
        drop(lease);
        std::fs::remove_dir(directory).unwrap();
        std::fs::remove_dir(base).unwrap();
    }
}
