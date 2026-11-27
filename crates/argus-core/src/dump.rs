//! Memory dump files of processes, as Task Manager's "Create memory dump
//! file" writes them: the whole address space, threads and modules, in the
//! temporary folder as `<image>.DMP`, `<image> (2).DMP` and so on.
//!
//! The dump is written from a snapshot (`PssCaptureSnapshot` with a clone of
//! the address space), so the process stops only while the snapshot is taken,
//! not for the whole write: a full dump of a large process takes seconds to
//! minutes. Where the snapshot is denied or fails, the process itself is
//! dumped and stays suspended while it is written.
//!
//! Handle names are left out of both. Reading a file handle's name waits on
//! the file object, and a handle with a synchronous read pending on a pipe
//! blocks the query until that read completes, possibly forever; the handle
//! stream of a snapshot dump still lists each handle and its type.

use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE, S_FALSE, WIN32_ERROR};
use windows::Win32::System::Diagnostics::Debug::{
    MINIDUMP_TYPE, MiniDumpIgnoreInaccessibleMemory, MiniDumpWithFullMemory,
    MiniDumpWithFullMemoryInfo, MiniDumpWithHandleData, MiniDumpWithThreadInfo,
    MiniDumpWithUnloadedModules,
};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, PROCESS_ACCESS_RIGHTS, PROCESS_CREATE_PROCESS, PROCESS_DUP_HANDLE,
    PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
};
use windows::core::{BOOL, s, w};

use crate::actions::{ActionError, image_name, open_instance};
use crate::util::OwnedHandle;

/// What the snapshot takes: a copy-on-write clone of the address space
/// (`PROCESS_CREATE_PROCESS`, `PROCESS_VM_OPERATION`), the threads and their
/// registers, and the handles (`PROCESS_DUP_HANDLE`).
const SNAPSHOT_ACCESS: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(
    PROCESS_QUERY_INFORMATION.0
        | PROCESS_VM_READ.0
        | PROCESS_VM_OPERATION.0
        | PROCESS_DUP_HANDLE.0
        | PROCESS_CREATE_PROCESS.0,
);
/// What dumping the process itself takes.
const DIRECT_ACCESS: PROCESS_ACCESS_RIGHTS =
    PROCESS_ACCESS_RIGHTS(PROCESS_QUERY_INFORMATION.0 | PROCESS_VM_READ.0);

// `PSS_CAPTURE_FLAGS` (processsnapshot.h): Microsoft's sample for dumping a
// snapshot, without `PSS_CAPTURE_HANDLE_NAME_INFORMATION` (see the module).
const PSS_CAPTURE_VA_CLONE: u32 = 0x0000_0001;
const PSS_CAPTURE_HANDLES: u32 = 0x0000_0004;
const PSS_CAPTURE_HANDLE_BASIC_INFORMATION: u32 = 0x0000_0010;
const PSS_CAPTURE_HANDLE_TYPE_SPECIFIC_INFORMATION: u32 = 0x0000_0020;
const PSS_CAPTURE_HANDLE_TRACE: u32 = 0x0000_0040;
const PSS_CAPTURE_THREADS: u32 = 0x0000_0080;
const PSS_CAPTURE_THREAD_CONTEXT: u32 = 0x0000_0100;
const PSS_CAPTURE_THREAD_CONTEXT_EXTENDED: u32 = 0x0000_0200;
const PSS_CREATE_BREAKAWAY_OPTIONAL: u32 = 0x0400_0000;
const PSS_CREATE_BREAKAWAY: u32 = 0x0800_0000;
const PSS_CREATE_USE_VM_ALLOCATIONS: u32 = 0x2000_0000;
const PSS_CREATE_RELEASE_SECTION: u32 = 0x8000_0000;
const SNAPSHOT_FLAGS: u32 = PSS_CAPTURE_VA_CLONE
    | PSS_CAPTURE_HANDLES
    | PSS_CAPTURE_HANDLE_BASIC_INFORMATION
    | PSS_CAPTURE_HANDLE_TYPE_SPECIFIC_INFORMATION
    | PSS_CAPTURE_HANDLE_TRACE
    | PSS_CAPTURE_THREADS
    | PSS_CAPTURE_THREAD_CONTEXT
    | PSS_CAPTURE_THREAD_CONTEXT_EXTENDED
    | PSS_CREATE_BREAKAWAY
    | PSS_CREATE_BREAKAWAY_OPTIONAL
    | PSS_CREATE_USE_VM_ALLOCATIONS
    | PSS_CREATE_RELEASE_SECTION;

/// `CONTEXT_ALL` of the machine Argus runs on: the snapshot's thread registers.
#[cfg(target_arch = "x86_64")]
const CONTEXT_ALL: u32 = windows::Win32::System::Diagnostics::Debug::CONTEXT_ALL_AMD64.0;
#[cfg(target_arch = "aarch64")]
const CONTEXT_ALL: u32 = windows::Win32::System::Diagnostics::Debug::CONTEXT_ALL_ARM64.0;
#[cfg(target_arch = "x86")]
const CONTEXT_ALL: u32 = windows::Win32::System::Diagnostics::Debug::CONTEXT_ALL_X86.0;

/// A full dump, as Task Manager and `procdump -ma` write: all memory and its
/// layout, threads with their times, and the modules unloaded so far.
const FULL: MINIDUMP_TYPE = MINIDUMP_TYPE(
    MiniDumpWithFullMemory.0
        | MiniDumpWithFullMemoryInfo.0
        | MiniDumpWithThreadInfo.0
        | MiniDumpWithUnloadedModules.0
        | MiniDumpIgnoreInaccessibleMemory.0,
);
/// A snapshot also lists the handles it captured, without their names.
const FULL_WITH_HANDLES: MINIDUMP_TYPE = MINIDUMP_TYPE(FULL.0 | MiniDumpWithHandleData.0);

/// `MINIDUMP_CALLBACK_TYPE` `IsProcessSnapshotCallback`.
const IS_PROCESS_SNAPSHOT_CALLBACK: u32 = 16;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn PssCaptureSnapshot(
        process: HANDLE,
        flags: u32,
        context_flags: u32,
        snapshot: *mut *mut c_void,
    ) -> u32;
    fn PssFreeSnapshot(process: HANDLE, snapshot: *mut c_void) -> u32;
}

/// `MiniDumpWriteDump` (dbghelp.h).
type WriteDump = unsafe extern "system" fn(
    process: HANDLE,
    pid: u32,
    file: HANDLE,
    kind: MINIDUMP_TYPE,
    exception: *const c_void,
    streams: *const c_void,
    callback: *const CallbackInformation,
) -> BOOL;

/// The leading fields of `MINIDUMP_CALLBACK_INPUT`, packed as dbghelp.h is.
#[repr(C, packed(4))]
struct CallbackInput {
    process_id: u32,
    process_handle: HANDLE,
    callback_type: u32,
}

/// `MINIDUMP_CALLBACK_INFORMATION`.
#[repr(C, packed(4))]
struct CallbackInformation {
    routine: unsafe extern "system" fn(*mut c_void, *const CallbackInput, *mut c_void) -> BOOL,
    param: *mut c_void,
}

/// Tells dbghelp that the handle it was given is a snapshot's. Every other
/// callback keeps dbghelp's defaults.
unsafe extern "system" fn snapshot_callback(
    _param: *mut c_void,
    input: *const CallbackInput,
    output: *mut c_void,
) -> BOOL {
    // `MINIDUMP_CALLBACK_OUTPUT` is a union: `Status` is its first member.
    if unsafe { std::ptr::addr_of!((*input).callback_type).read_unaligned() }
        == IS_PROCESS_SNAPSHOT_CALLBACK
    {
        unsafe { output.cast::<i32>().write_unaligned(S_FALSE.0) };
    }
    BOOL(1)
}

/// dbghelp, loaded from System32 the first time a dump is written. Its
/// functions are not safe to call from two threads at once.
fn writer() -> Result<WriteDump, ActionError> {
    static WRITER: OnceLock<Result<usize, windows::core::Error>> = OnceLock::new();
    let address = WRITER.get_or_init(|| unsafe {
        let module = LoadLibraryExW(w!("dbghelp.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)?;
        GetProcAddress(module, s!("MiniDumpWriteDump"))
            .map(|address| address as usize)
            .ok_or_else(windows::core::Error::from_thread)
    });
    match address {
        Ok(address) => Ok(unsafe { std::mem::transmute::<usize, WriteDump>(*address) }),
        Err(error) => {
            crate::diagnostics::windows_failure("memory dump load dbghelp", error);
            Err(error.clone().into())
        }
    }
}
static DBGHELP: Mutex<()> = Mutex::new(());

/// A process snapshot, freed when dropped.
struct Snapshot(*mut c_void);

impl Snapshot {
    fn capture(process: &OwnedHandle) -> Result<Self, WIN32_ERROR> {
        let mut snapshot = std::ptr::null_mut();
        let status =
            unsafe { PssCaptureSnapshot(process.0, SNAPSHOT_FLAGS, CONTEXT_ALL, &mut snapshot) };
        if status == ERROR_SUCCESS.0 {
            Ok(Self(snapshot))
        } else {
            Err(WIN32_ERROR(status))
        }
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        // Freeing also ends the clone of the address space.
        unsafe { PssFreeSnapshot(GetCurrentProcess(), self.0) };
    }
}

/// The image name Task Manager names a dump after, without its extension.
fn stem(image: &str) -> &str {
    let stem = Path::new(image)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    if stem.is_empty() { "process" } else { stem }
}

/// The `n`th name for a dump of `image`, counting from 1.
fn file_name(image: &str, n: u32) -> String {
    match n {
        1 => format!("{}.DMP", stem(image)),
        n => format!("{} ({n}).DMP", stem(image)),
    }
}

/// Creates the first free dump file for `image` in `dir`.
fn reserve(dir: &Path, image: &str) -> io::Result<(PathBuf, File)> {
    for n in 1..=1000 {
        let path = dir.join(file_name(image, n));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::ErrorKind::AlreadyExists.into())
}

/// The Local Security Authority keeps the system's sign-in secrets in its
/// memory. Security software treats a program reading it as credential theft
/// and may quarantine that program, so it is never opened for reading.
pub fn holds_credentials(image: &str) -> bool {
    image.eq_ignore_ascii_case("lsass.exe")
}

/// Writes a full memory dump of the process instance `pid` + `create_time`
/// to the temporary folder, as Task Manager does; returns the file's path.
/// Acts with the caller's rights: another account's or an elevated process
/// needs an elevated caller, and protected processes cannot be dumped.
pub fn create(pid: u32, create_time: i64) -> Result<PathBuf, ActionError> {
    create_in(&std::env::temp_dir(), pid, create_time)
}

fn create_in(dir: &Path, pid: u32, create_time: i64) -> Result<PathBuf, ActionError> {
    // The System process and Idle have no user-mode memory to dump.
    if pid == 0 || pid == 4 {
        return Err(ActionError::Refused);
    }
    // Named before any handle that reads memory is asked for.
    let image = image_name(open_instance(pid, create_time, PROCESS_ACCESS_RIGHTS(0))?.0)?;
    if holds_credentials(&image) {
        return Err(ActionError::Refused);
    }
    let write = writer()?;
    let (path, file) = reserve(dir, &image).map_err(|error| {
        crate::diagnostics::warn(
            "memory dump create file",
            format!(
                "{}; dir={}",
                crate::diagnostics::io(&error),
                crate::diagnostics::path(dir)
            ),
        );
        ActionError::Os(error.to_string())
    })?;
    let result = {
        let _serial = DBGHELP
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        dump(write, pid, create_time, &file)
    };
    drop(file);
    if result.is_err() {
        let _ = std::fs::remove_file(&path);
    }
    result.map(|()| path)
}

fn dump(write: WriteDump, pid: u32, create_time: i64, file: &File) -> Result<(), ActionError> {
    let process = match open_instance(pid, create_time, SNAPSHOT_ACCESS) {
        Ok(process) => process,
        Err(ActionError::AccessDenied) => {
            return direct(
                write,
                &open_instance(pid, create_time, DIRECT_ACCESS)?,
                pid,
                file,
            );
        }
        Err(error) => return Err(error),
    };
    let snapshot = match Snapshot::capture(&process) {
        Ok(snapshot) => snapshot,
        Err(status) => {
            crate::diagnostics::warn(
                "memory dump PssCaptureSnapshot",
                crate::diagnostics::code(status.0),
            );
            return direct(write, &process, pid, file);
        }
    };
    let callback = CallbackInformation {
        routine: snapshot_callback,
        param: std::ptr::null_mut(),
    };
    let written = unsafe {
        write(
            HANDLE(snapshot.0),
            pid,
            HANDLE(file.as_raw_handle()),
            FULL_WITH_HANDLES,
            std::ptr::null(),
            std::ptr::null(),
            &callback,
        )
    };
    finished("memory dump MiniDumpWriteDump snapshot", written)
}

/// Dumps the process itself, suspended meanwhile.
fn direct(
    write: WriteDump,
    process: &OwnedHandle,
    pid: u32,
    file: &File,
) -> Result<(), ActionError> {
    let written = unsafe {
        write(
            process.0,
            pid,
            HANDLE(file.as_raw_handle()),
            FULL,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    finished("memory dump MiniDumpWriteDump", written)
}

fn finished(event: &'static str, written: BOOL) -> Result<(), ActionError> {
    if written.as_bool() {
        return Ok(());
    }
    // dbghelp leaves an HRESULT as the thread's last error.
    let error = windows::core::Error::from_thread();
    crate::diagnostics::windows_failure(event, &error);
    Err(error.into())
}

#[cfg(test)]
mod tests {
    use super::{
        DBGHELP, DIRECT_ACCESS, create_in, direct, file_name, holds_credentials, reserve, writer,
    };
    use crate::actions::open_instance;
    use std::os::windows::process::CommandExt;

    #[test]
    fn dumps_are_named_after_the_image_and_numbered_from_the_second() {
        assert_eq!(file_name("notepad.exe", 1), "notepad.DMP");
        assert_eq!(file_name("notepad.exe", 2), "notepad (2).DMP");
        assert_eq!(
            file_name("Code - Insiders.exe", 3),
            "Code - Insiders (3).DMP"
        );
        assert_eq!(file_name("Registry", 1), "Registry.DMP");
        assert_eq!(file_name("", 1), "process.DMP");
    }

    #[test]
    fn a_dump_never_replaces_an_earlier_one() {
        let dir = std::env::temp_dir().join(format!("argus-dump-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (first, _a) = reserve(&dir, "app.exe").unwrap();
        let (second, _b) = reserve(&dir, "APP.EXE").unwrap();
        assert_eq!(first.file_name().unwrap(), "app.DMP");
        assert_eq!(second.file_name().unwrap(), "APP (2).DMP");
        drop((_a, _b));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_the_local_security_authority_is_kept_from_dumps() {
        assert!(holds_credentials("lsass.exe"));
        assert!(holds_credentials("LSASS.EXE"));
        assert!(!holds_credentials("lsaiso.exe"));
        assert!(!holds_credentials("svchost.exe"));
    }

    /// A child of the test that keeps running for half a minute.
    fn child() -> std::process::Child {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new(std::env::var("ComSpec").unwrap())
            .args(["/d", "/c", "ping -n 30 127.0.0.1 >nul"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap()
    }

    /// The kinds of the streams a minidump file holds (`MINIDUMP_HEADER`:
    /// the signature, the stream count, then where their directory starts).
    fn streams(path: &std::path::Path) -> Vec<u32> {
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(&bytes[..4], b"MDMP");
        let count = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let directory = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        (0..count as usize)
            .map(|i| {
                let at = directory as usize + i * 12;
                u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
            })
            .collect()
    }

    // ThreadListStream, ModuleListStream, Memory64ListStream, HandleDataStream.
    const THREADS: u32 = 3;
    const MODULES: u32 = 4;
    const MEMORY: u32 = 9;
    const HANDLES: u32 = 12;

    /// Dumped from a snapshot: the file is a minidump with memory, threads
    /// and handles, and the child keeps running.
    #[test]
    fn a_running_process_is_dumped_to_a_minidump_file() {
        let mut child = child();
        let dir = std::env::temp_dir().join(format!("argus-dump-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let started = std::time::Instant::now();
        let result = create_in(&dir, child.id(), 0);
        let elapsed = started.elapsed();
        let still_running = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let _ = child.wait();
        let path = result.unwrap();
        assert!(still_running);
        assert_eq!(path.file_name().unwrap(), "cmd.DMP");
        let kinds = streams(&path);
        for kind in [THREADS, MODULES, MEMORY, HANDLES] {
            assert!(
                kinds.contains(&kind),
                "stream {kind} missing from {kinds:?}"
            );
        }
        eprintln!(
            "dumped {} bytes in {elapsed:?}",
            std::fs::metadata(&path).unwrap().len()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The fallback where no snapshot can be taken: the process itself,
    /// without its handles.
    #[test]
    fn a_process_is_dumped_directly_without_a_snapshot() {
        let mut child = child();
        let pid = child.id();
        let dir = std::env::temp_dir().join(format!("argus-dump-direct-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (path, file) = reserve(&dir, "cmd.exe").unwrap();
        let process = open_instance(pid, 0, DIRECT_ACCESS).unwrap();
        let result = {
            // dbghelp is called one thread at a time, as `create_in` does.
            let _serial = DBGHELP
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            direct(writer().unwrap(), &process, pid, &file)
        };
        drop(file);
        let _ = child.kill();
        let _ = child.wait();
        result.unwrap();
        let kinds = streams(&path);
        for kind in [THREADS, MODULES, MEMORY] {
            assert!(
                kinds.contains(&kind),
                "stream {kind} missing from {kinds:?}"
            );
        }
        assert!(!kinds.contains(&HANDLES));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_instance_that_is_gone_is_not_dumped() {
        let dir = std::env::temp_dir();
        assert!(matches!(
            create_in(&dir, std::process::id(), 1),
            Err(crate::actions::ActionError::NotFound)
        ));
        assert!(matches!(
            create_in(&dir, 4, 0),
            Err(crate::actions::ActionError::Refused)
        ));
    }

    /// How far dumps reach with the rights of the account running the test,
    /// in a disposable VM (`ARGUS_VM_TEST=1`). An elevated test enables
    /// SeDebugPrivilege first, as an elevated Argus does at start.
    /// `ARGUS_DUMP_EXPECT` lists `pid=outcome` pairs, comma-separated; an
    /// outcome is `snapshot`, `direct` or an `ActionError` kind such as
    /// `accessDenied`. The test's own child is dumped from a snapshot in any
    /// case. Every result is printed before any is checked.
    #[test]
    #[ignore = "dumps processes named by an explicitly configured disposable VM"]
    fn vm_dump_reach() {
        assert_eq!(std::env::var("ARGUS_VM_TEST").as_deref(), Ok("1"));
        let elevated = crate::privilege::is_elevated();
        let debug = elevated && crate::privilege::enable_debug_privilege();
        println!("elevated={elevated}; SeDebugPrivilege={debug}");
        let dir = std::env::temp_dir().join(format!("argus-vm-dumps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut own = child();
        let mut expected = vec![(own.id(), "snapshot".to_owned())];
        let pairs = std::env::var("ARGUS_DUMP_EXPECT").unwrap_or_default();
        for pair in pairs.split(',').filter(|pair| !pair.is_empty()) {
            let (pid, outcome) = pair.split_once('=').expect("pid=outcome");
            expected.push((pid.trim().parse().unwrap(), outcome.trim().to_owned()));
        }
        let results: Vec<(u32, String, String)> = expected
            .into_iter()
            .map(|(pid, want)| {
                let got = match create_in(&dir, pid, 0) {
                    Ok(path) if streams(&path).contains(&HANDLES) => "snapshot".to_owned(),
                    Ok(_) => "direct".to_owned(),
                    Err(error) => serde_json::to_value(&error).unwrap()["kind"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                };
                println!("pid {pid}: {got} (expected {want})");
                (pid, want, got)
            })
            .collect();
        let _ = own.kill();
        let _ = own.wait();
        std::fs::remove_dir_all(&dir).unwrap();
        for (pid, want, got) in results {
            assert_eq!(got, want, "pid {pid}");
        }
    }
}
