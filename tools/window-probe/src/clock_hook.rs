//! Black-box clock injection into the probe's own isolated test child only.
use anyhow::{Context, Result, ensure};
use std::{
    ffi::c_void,
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};
use windows::{
    Win32::{
        Foundation::{FreeLibrary, HANDLE, HMODULE, WAIT_OBJECT_0},
        System::{
            Diagnostics::{Debug::WriteProcessMemory, ToolHelp::*},
            LibraryLoader::*,
            Memory::*,
            Threading::*,
        },
    },
    core::{PCWSTR, s, w},
};

fn remote_module(pid: u32, name: &str) -> Result<usize> {
    unsafe {
        let snapshot =
            OwnedHandle::from_raw_handle(CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid)?.0);
        let mut entry = MODULEENTRY32W {
            dwSize: size_of::<MODULEENTRY32W>() as u32,
            ..Default::default()
        };
        Module32FirstW(HANDLE(snapshot.as_raw_handle()), &mut entry)?;
        loop {
            let length = entry
                .szModule
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szModule.len());
            if String::from_utf16_lossy(&entry.szModule[..length]).eq_ignore_ascii_case(name) {
                return Ok(entry.modBaseAddr as usize);
            }
            if Module32NextW(HANDLE(snapshot.as_raw_handle()), &mut entry).is_err() {
                break;
            }
        }
    }
    anyhow::bail!("clock injection module unavailable: {name}")
}

unsafe fn call(process: HANDLE, function: usize, parameter: Option<*const c_void>) -> Result<u32> {
    unsafe {
        let thread = OwnedHandle::from_raw_handle(
            CreateRemoteThread(
                process,
                None,
                0,
                Some(std::mem::transmute::<
                    usize,
                    unsafe extern "system" fn(*mut c_void) -> u32,
                >(function)),
                parameter,
                0,
                None,
            )?
            .0,
        );
        ensure!(
            WaitForSingleObject(HANDLE(thread.as_raw_handle()), 10000) == WAIT_OBJECT_0,
            "owned clock injection timed out"
        );
        let mut code = 0;
        GetExitCodeThread(HANDLE(thread.as_raw_handle()), &mut code)?;
        Ok(code)
    }
}

pub fn install(process: HANDLE, pid: u32, args: &[String]) -> Result<u32> {
    ensure!(
        args.iter().any(|v| v == "--test")
            && args
                .windows(2)
                .any(|v| v[0] == "--identifier" && v[1].starts_with("dev.argus.taskmanager.test.")),
        "clock hook requires an isolated test child"
    );
    let path = std::fs::canonicalize(std::env::var("ARGUS_PROBE_CLOCK_DLL")?)?;
    let name = path
        .file_name()
        .context("clock DLL name")?
        .to_string_lossy();
    let wide: Vec<u16> = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        let loader = GetProcAddress(GetModuleHandleW(w!("kernel32.dll"))?, s!("LoadLibraryW"))
            .context("LoadLibraryW")? as usize;
        // Resolve the containing module because forwarded exports can live in KernelBase.
        let mut module = HMODULE::default();
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(loader as *const u16),
            &mut module,
        )?;
        let mut filename = [0u16; 32768];
        let length = GetModuleFileNameW(Some(module), &mut filename) as usize;
        let filename = String::from_utf16_lossy(&filename[..length]);
        let module_name = filename.rsplit('\\').next().context("loader module name")?;
        let remote_loader = remote_module(pid, module_name)? + loader - module.0 as usize;
        let bytes = wide.len() * size_of::<u16>();
        let buffer = VirtualAllocEx(
            process,
            None,
            bytes,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        ensure!(!buffer.is_null(), "owned clock path allocation failed");
        WriteProcessMemory(process, buffer, wide.as_ptr().cast(), bytes, None)?;
        // On timeout the owned job is terminated; do not free a path still in use.
        call(process, remote_loader, Some(buffer))?;
        VirtualFreeEx(process, buffer, 0, MEM_RELEASE)?;
        let remote = remote_module(pid, &name)?;
        let local = LoadLibraryExW(PCWSTR(wide.as_ptr()), None, DONT_RESOLVE_DLL_REFERENCES)?;
        let entry = GetProcAddress(local, s!("ArgusInstallClock"))
            .map(|entry| entry as usize - local.0 as usize);
        FreeLibrary(local)?;
        let count = call(process, remote + entry.context("clock hook entry")?, None)?;
        ensure!(
            count >= 2,
            "clock hook did not replace the required imports"
        );
        Ok(count)
    }
}
