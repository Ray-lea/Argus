use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS};
use windows::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};
use windows::Win32::System::Registry::*;
use windows::core::{PCWSTR, PWSTR};

use super::state::{Journal, Observation, Result, Store, Value};
use crate::{security, util};

pub const IFEO: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options";
pub const KEY: &str =
    r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\taskmgr.exe";
pub const JOURNAL: &str = "taskmgr-recovery.json";
pub struct Key(pub HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn err(code: windows::Win32::Foundation::WIN32_ERROR) -> Result<()> {
    if code.0 == 0 {
        Ok(())
    } else {
        Err(util::message(code.0))
    }
}

pub fn open(path: &str, write: bool) -> Result<Option<Key>> {
    let mut key = HKEY::default();
    let code = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(util::wide(path).as_ptr()),
            None,
            KEY_READ
                | if write {
                    KEY_SET_VALUE
                } else {
                    Default::default()
                },
            &mut key,
        )
    };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    err(code)?;
    Ok(Some(Key(key)))
}

pub fn read_value(key: &Key, name: &str) -> Result<Option<Value>> {
    let name = util::wide(name);
    let mut kind = REG_VALUE_TYPE::default();
    let mut size = 0;
    let code = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut size),
        )
    };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    err(code)?;
    if size > 16 * 1024 {
        return Err("注册表值过长，已停止操作".into());
    }
    let mut bytes = vec![0; size as usize];
    err(unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            Some(bytes.as_mut_ptr()),
            Some(&mut size),
        )
    })?;
    bytes.truncate(size as usize);
    Ok(Some(Value {
        kind: kind.0,
        bytes,
    }))
}

pub fn observe(path: &str) -> Result<Observation> {
    let Some(key) = open(path, false)? else {
        return Ok(Observation::default());
    };
    let mut name = [0u16; 256];
    let mut len = name.len() as u32;
    let code = unsafe {
        RegEnumKeyExW(
            key.0,
            0,
            Some(PWSTR(name.as_mut_ptr())),
            &mut len,
            None,
            None,
            None,
            None,
        )
    };
    if code != ERROR_NO_MORE_ITEMS {
        err(code)?;
    }
    Ok(Observation {
        debugger: read_value(&key, "Debugger")?,
        filtered: read_value(&key, "UseFilter")?.is_some() || code != ERROR_NO_MORE_ITEMS,
    })
}

pub fn write(path: &str, value: Option<&Value>) -> Result<()> {
    let key = if let Some(key) = open(path, true)? {
        key
    } else {
        if value.is_none() {
            return Ok(());
        }
        let mut raw = HKEY::default();
        err(unsafe {
            RegCreateKeyExW(
                HKEY_LOCAL_MACHINE,
                PCWSTR(util::wide(path).as_ptr()),
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_READ | KEY_SET_VALUE,
                None,
                &mut raw,
                None,
            )
        })?;
        Key(raw)
    };
    let name = util::wide("Debugger");
    let code = match value {
        Some(value) => unsafe {
            RegSetValueExW(
                key.0,
                PCWSTR(name.as_ptr()),
                None,
                REG_VALUE_TYPE(value.kind),
                Some(&value.bytes),
            )
        },
        None => unsafe { RegDeleteValueW(key.0, PCWSTR(name.as_ptr())) },
    };
    if code != ERROR_FILE_NOT_FOUND {
        err(code)?;
    }
    err(unsafe { RegFlushKey(key.0) })
}

pub struct Machine {
    pub dir: PathBuf,
}
impl Machine {
    pub fn load(&self) -> Result<Option<Journal>> {
        let path = self.dir.join(JOURNAL);
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
            Ok(meta) if meta.len() > 64 * 1024 => return Err("恢复记录过长".into()),
            _ => {}
        }
        let _file = security::protect_directory(&path).map_err(util::message)?;
        let journal: Journal = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|_| "恢复记录损坏，请使用应急恢复说明".to_string())?;
        if journal.version != 1
            || journal.before != Observation::default()
            || journal.replacement != super::replacement(&self.dir)
            || !super::valid_image(&journal.image)
        {
            return Err("恢复记录与此安装不符，已停止操作".into());
        }
        security::validate_sid(&journal.owner).map_err(util::message)?;
        Ok(Some(journal))
    }
}
impl Store for Machine {
    fn observe(&mut self) -> Result<Observation> {
        observe(KEY)
    }
    fn write_debugger(&mut self, value: Option<&Value>) -> Result<()> {
        write(KEY, value)
    }
    fn save(&mut self, journal: &Journal) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(journal).map_err(|e| e.to_string())?;
        atomic_file(&self.dir, JOURNAL, &bytes)
    }
}

pub fn atomic_file(dir: &Path, filename: &str, bytes: &[u8]) -> Result<()> {
    let destination = dir.join(filename);
    if destination.exists() {
        drop(security::protect_directory(&destination).map_err(util::message)?);
    }
    let temporary = dir.join(format!("taskmgr-stage-{}.tmp", super::id()?));
    let result = (|| {
        let mut file = crate::install::new_protected_file(&temporary).map_err(util::message)?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        unsafe {
            MoveFileExW(
                PCWSTR(util::wide(&temporary.to_string_lossy()).as_ptr()),
                PCWSTR(util::wide(&destination.to_string_lossy()).as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
