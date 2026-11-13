//! A watched program is identified from the validated process handle, never a UI label.
use super::WatchError;
use crate::util::{OwnedHandle, filetime_ticks, ticks_to_unix_ms};
use serde::{Deserialize, Serialize};
use windows::{
    Win32::{
        Foundation::{FILETIME, WAIT_TIMEOUT},
        System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_NAME_FORMAT, PROCESS_NAME_NATIVE,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW,
            WaitForSingleObject,
        },
    },
    core::PWSTR,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProgramIdentity {
    /// The native image path avoids conflating two executables with the same basename.
    pub native_path: String,
    pub display_path: String,
}

impl ProgramIdentity {
    pub fn valid(&self) -> bool {
        [&self.native_path, &self.display_path].iter().all(|path| {
            !path.is_empty() && path.encode_utf16().count() < 32768 && !path.contains('\0')
        })
    }

    /// Exact native identity is deliberately conservative around case-sensitive directories.
    pub fn same_program(&self, other: &Self) -> bool {
        self.native_path == other.native_path
    }
}

pub(super) struct Instance {
    handle: OwnedHandle,
}

fn error(error: windows::core::Error) -> WatchError {
    match error.code().0 as u32 {
        0x80070005 => WatchError::Access,
        _ => WatchError::Identity,
    }
}

impl Instance {
    pub(super) fn open(pid: u32, created: i64) -> Result<Self, WatchError> {
        if pid == 0 || created <= 0 {
            return Err(WatchError::Identity);
        }
        let handle = OwnedHandle(
            unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    false,
                    pid,
                )
            }
            .map_err(error)?,
        );
        let (mut birth, mut exited, mut kernel, mut user) = Default::default();
        unsafe { GetProcessTimes(handle.0, &mut birth, &mut exited, &mut kernel, &mut user) }
            .map_err(error)?;
        let birth: FILETIME = birth;
        if ticks_to_unix_ms(filetime_ticks(birth)) != created
            || unsafe { WaitForSingleObject(handle.0, 0) } != WAIT_TIMEOUT
        {
            return Err(WatchError::Identity);
        }
        Ok(Self { handle })
    }

    pub(super) fn program(&self) -> Result<ProgramIdentity, WatchError> {
        fn image(handle: &OwnedHandle, flags: PROCESS_NAME_FORMAT) -> Result<String, WatchError> {
            let mut buffer = vec![0u16; 32768];
            let mut length = buffer.len() as u32;
            unsafe {
                QueryFullProcessImageNameW(handle.0, flags, PWSTR(buffer.as_mut_ptr()), &mut length)
            }
            .map_err(error)?;
            String::from_utf16(&buffer[..length as usize]).map_err(|_| WatchError::Identity)
        }
        let identity = ProgramIdentity {
            native_path: image(&self.handle, PROCESS_NAME_NATIVE)?,
            display_path: image(&self.handle, PROCESS_NAME_FORMAT(0))?,
        };
        if !identity.valid() || unsafe { WaitForSingleObject(self.handle.0, 0) } != WAIT_TIMEOUT {
            return Err(WatchError::Identity);
        }
        Ok(identity)
    }

    pub(super) fn into_handle(self) -> OwnedHandle {
        self.handle
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_and_case_sensitive_paths_are_not_program_identity() {
        let identity = |path: &str| ProgramIdentity {
            native_path: path.into(),
            display_path: String::new(),
        };
        assert!(
            !identity("\\Device\\disk\\one\\same.exe")
                .same_program(&identity("\\Device\\disk\\two\\same.exe"))
        );
        assert!(
            !identity("\\Device\\disk\\Folder\\same.exe")
                .same_program(&identity("\\Device\\disk\\folder\\same.exe"))
        );
        assert!(Instance::open(std::process::id(), 0).is_err());
    }

    #[test]
    #[ignore = "owned hidden child images in two temporary folders; run explicitly"]
    fn two_copies_stay_distinct_and_restarting_the_same_copy_keeps_program_identity() {
        use std::{
            os::windows::{io::AsRawHandle, process::CommandExt},
            process::{Child, Command, Stdio},
        };
        use windows::Win32::Foundation::HANDLE;
        fn birth(child: &Child) -> i64 {
            let (mut created, mut exited, mut kernel, mut user) = Default::default();
            unsafe {
                GetProcessTimes(
                    HANDLE(child.as_raw_handle()),
                    &mut created,
                    &mut exited,
                    &mut kernel,
                    &mut user,
                )
            }
            .unwrap();
            ticks_to_unix_ms(filetime_ticks(created))
        }
        fn start(path: &std::path::Path) -> Child {
            Command::new(path)
                .args(["--exact", "process::tests::liveness_child", "--ignored"])
                .creation_flags(0x08000000)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        }
        fn exit(mut child: Child) {
            drop(child.stdin.take());
            assert!(child.wait().unwrap().success());
        }
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "argus-program-identity-{}-{suffix}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        for name in ["one", "two"] {
            std::fs::create_dir(dir.join(name)).unwrap();
            std::fs::copy(
                std::env::current_exe().unwrap(),
                dir.join(name).join("same.exe"),
            )
            .unwrap();
        }
        let one = dir.join("one/same.exe");
        let two = dir.join("two/same.exe");
        let a = start(&one);
        let b = start(&two);
        let first = Instance::open(a.id(), birth(&a)).unwrap();
        let second = Instance::open(b.id(), birth(&b)).unwrap();
        let a_program = first.program().unwrap();
        assert!(!a_program.same_program(&second.program().unwrap()));
        let first_pid = a.id();
        exit(a);
        let again = start(&one);
        assert_ne!(again.id(), first_pid);
        let restarted = Instance::open(again.id(), birth(&again)).unwrap();
        assert!(a_program.same_program(&restarted.program().unwrap()));
        assert!(Instance::open(again.id(), birth(&again) + 1000).is_err());
        exit(again);
        exit(b);
        drop((first, second, restarted));
        for path in [&one, &two] {
            std::fs::remove_file(path).unwrap();
        }
        for name in ["one", "two"] {
            std::fs::remove_dir(dir.join(name)).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }
}
