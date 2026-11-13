//! Common file picker checks run only on the probe's inactive desktop, never the user's desktop.
use super::{Format, PreviewStore};
use std::{
    os::windows::ffi::OsStrExt,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HANDLE, HWND, LPARAM, WPARAM},
        System::{
            StationsAndDesktops::{GetThreadDesktop, GetUserObjectInformationW, UOI_NAME},
            Threading::GetCurrentThreadId,
        },
        UI::WindowsAndMessaging::*,
    },
    core::BOOL,
};

struct Search {
    title: String,
    filename: String,
    dialog: Option<HWND>,
    edit: Option<HWND>,
}
unsafe extern "system" fn child(hwnd: HWND, data: LPARAM) -> BOOL {
    let search = unsafe { &mut *(data.0 as *mut Search) };
    let mut class = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
    if String::from_utf16_lossy(&class[..len]) != "Edit" {
        return true.into();
    }
    let mut value = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut value) } as usize;
    if String::from_utf16_lossy(&value[..len]) == search.filename {
        search.edit = Some(hwnd);
        return false.into();
    }
    true.into()
}
unsafe extern "system" fn top(hwnd: HWND, data: LPARAM) -> BOOL {
    let search = unsafe { &mut *(data.0 as *mut Search) };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid != std::process::id() {
        return true.into();
    }
    let mut class = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
    if String::from_utf16_lossy(&class[..len]) != "#32770" {
        return true.into();
    }
    let mut title = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut title) } as usize;
    if String::from_utf16_lossy(&title[..len]) != search.title {
        return true.into();
    }
    search.dialog = Some(hwnd);
    unsafe {
        let _ = EnumChildWindows(Some(hwnd), Some(child), data);
    }
    search.edit.is_none().into()
}

#[test]
#[ignore = "must run through argus-window-probe startup on an inactive desktop"]
fn native_save_dialog_saves_exact_preview_and_cancel_writes_nothing() {
    let desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) }.unwrap();
    let mut name = [0u16; 256];
    unsafe {
        GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            (name.len() * 2) as u32,
            None,
        )
    }
    .unwrap();
    let name = String::from_utf16_lossy(
        &name[..name
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(name.len())],
    );
    assert!(
        name.starts_with("ArgusStartup-"),
        "refusing to open a picker outside the probe's private desktop"
    );
    let dir = super::tests::folder();
    let report = argus_core::history::interval(
        &dir,
        1_700_000_000_000,
        1_700_000_010_000,
        argus_core::history::IntervalMetric::Cpu,
    )
    .unwrap();
    let host = crate::native_runtime::Host::new(format!(
        "dev.argus.taskmanager.test.report-dialog-{}",
        std::process::id()
    ));
    host.manage(PreviewStore::default());
    for (format, cancel) in [
        (Format::Markdown, false),
        (Format::Csv, false),
        (Format::Csv, true),
    ] {
        let preview = host
            .state::<PreviewStore>()
            .prepare(&report, format)
            .unwrap();
        let target = dir.join(if cancel {
            "never-written.csv"
        } else {
            match format {
                Format::Markdown => "chosen.md",
                Format::Csv => "chosen.csv",
            }
        });
        let destination = target.clone();
        let extension = format.extension();
        let helper = std::thread::spawn(move || {
            let mut search = Search {
                title: argus_core::tr!("history.report.save"),
                filename: format!("argus-report.{extension}"),
                dialog: None,
                edit: None,
            };
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                unsafe {
                    let _ = EnumWindows(Some(top), LPARAM((&mut search as *mut Search) as isize));
                }
                if let (Some(dialog), Some(edit)) = (search.dialog, search.edit) {
                    if cancel {
                        unsafe { PostMessageW(Some(dialog), WM_CLOSE, WPARAM(0), LPARAM(0)) }
                            .unwrap();
                    } else {
                        let path: Vec<u16> = destination
                            .as_os_str()
                            .encode_wide()
                            .chain(Some(0))
                            .collect();
                        unsafe {
                            SendMessageW(
                                edit,
                                WM_SETTEXT,
                                Some(WPARAM(0)),
                                Some(LPARAM(path.as_ptr() as isize)),
                            );
                            PostMessageW(Some(dialog), WM_COMMAND, WPARAM(1), LPARAM(0)).unwrap();
                        }
                    }
                    return;
                }
                if Instant::now() > deadline {
                    if let Some(dialog) = search.dialog {
                        unsafe {
                            let _ = PostMessageW(Some(dialog), WM_CLOSE, WPARAM(0), LPARAM(0));
                        }
                    }
                    panic!("owned Save dialog or its filename field was not found");
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let saved =
            crate::commands::history_report_save(host.state(), preview.id, preview.sha256.clone())
                .unwrap();
        helper.join().unwrap();
        assert_eq!(saved, !cancel);
        if cancel {
            assert!(!target.exists());
        } else {
            assert_eq!(std::fs::read(&target).unwrap(), preview.body.as_bytes());
        }
    }
    if let Some(output) = std::env::var_os("ARGUS_TEST_REPORT_DIALOG_OUTPUT") {
        std::fs::write(output,br#"{"inactiveDesktop":true,"markdownExact":true,"csvExact":true,"cancelWroteNothing":true}"#).unwrap();
    }
    std::fs::remove_dir_all(dir).unwrap();
}
