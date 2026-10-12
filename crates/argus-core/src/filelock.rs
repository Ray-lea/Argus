//! "Who is using this file?" through the Restart Manager, which finds the
//! processes holding a file open (or a DLL loaded) without administrator
//! rights, as long as we may query those processes.

use std::path::{Path, PathBuf};

use serde::Serialize;
use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::RestartManager::{
    CCH_RM_SESSION_KEY, RM_PROCESS_INFO, RmEndSession, RmGetList, RmRegisterResources,
    RmStartSession,
};
use windows::core::{PCWSTR, PWSTR};

use crate::actions::ActionError;
use crate::util::{filetime_ticks, from_wide, ticks_to_unix_ms, wide};

/// Folders are checked file by file; cap the walk so a drive root stays responsive.
const MAX_FILES: usize = 20_000;
const REGISTER_BATCH: usize = 500;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Locker {
    pub pid: u32,
    /// Unix ms, to tell a reused PID apart.
    pub create_time: i64,
    pub app_name: String,
    pub service: Option<String>,
    /// `window` | `service` | `explorer` | `console` | `critical` | `other`
    pub kind: &'static str,
    pub handle_count: usize,
    pub handle_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockReport {
    pub lockers: Vec<Locker>,
    pub files_checked: usize,
    /// Files the Restart Manager refused to inspect (typically system files
    /// loaded by protected processes).
    pub unchecked: usize,
    /// The folder held more files than were checked.
    pub truncated: bool,
    pub handles: Option<crate::handles::HandleSummary>,
    pub restart_error: Option<String>,
}

impl LockReport {
    pub fn combine(
        base: Result<Self, ActionError>,
        handles: crate::handles::HandleReport,
    ) -> Result<Self, ActionError> {
        let mut report = match base {
            Ok(report) => report,
            Err(_) if handles.summary.available => Self {
                lockers: Vec::new(),
                files_checked: 0,
                unchecked: 0,
                truncated: false,
                handles: None,
                restart_error: Some("Restart Manager 无法检查目标，以下为句柄查询结果".into()),
            },
            Err(error) => return Err(error),
        };
        report.merge_handles(handles);
        Ok(report)
    }
    pub fn merge_handles(&mut self, report: crate::handles::HandleReport) {
        for hit in report.lockers {
            if let Some(locker) = self
                .lockers
                .iter_mut()
                .find(|l| l.pid == hit.pid && l.create_time == hit.create_time)
            {
                locker.handle_count += hit.count;
                locker.handle_path = Some(hit.path);
            } else {
                self.lockers.push(Locker {
                    pid: hit.pid,
                    create_time: hit.create_time,
                    app_name: hit.name,
                    service: None,
                    kind: "other",
                    handle_count: hit.count,
                    handle_path: Some(hit.path),
                });
            }
        }
        self.lockers.sort_by_key(|l| (l.pid, l.create_time));
        self.handles = Some(report.summary);
    }
}

struct Session(u32);

impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            let _ = RmEndSession(self.0);
        }
    }
}

pub fn find_lockers(paths: &[String]) -> Result<LockReport, ActionError> {
    let mut files = Vec::new();
    let mut truncated = false;
    for p in paths {
        let path = Path::new(p);
        if !path.exists() {
            return Err(ActionError::NotFound);
        }
        if path.is_dir() {
            truncated |= walk(path, &mut files);
        } else {
            files.push(path.to_path_buf());
        }
        if files.len() >= MAX_FILES {
            truncated = true;
            files.truncate(MAX_FILES);
            break;
        }
    }

    let wides: Vec<Vec<u16>> = files.iter().map(|f| wide(&f.to_string_lossy())).collect();
    let mut infos = Vec::new();
    let mut unchecked = 0;
    query_split(&wides, &mut infos, &mut unchecked);

    let mut lockers: Vec<Locker> = infos
        .iter()
        .map(|i| Locker {
            pid: i.Process.dwProcessId,
            create_time: ticks_to_unix_ms(filetime_ticks(i.Process.ProcessStartTime)),
            app_name: from_wide(&i.strAppName),
            service: Some(from_wide(&i.strServiceShortName)).filter(|s| !s.is_empty()),
            kind: match i.ApplicationType.0 {
                1 | 2 => "window",
                3 => "service",
                4 => "explorer",
                5 => "console",
                1000 => "critical",
                _ => "other",
            },
            handle_count: 0,
            handle_path: None,
        })
        .collect();
    lockers.sort_by_key(|l| (l.pid, l.create_time));
    lockers.dedup_by_key(|l| (l.pid, l.create_time));
    Ok(LockReport {
        lockers,
        files_checked: files.len() - unchecked,
        unchecked,
        truncated,
        handles: None,
        restart_error: None,
    })
}

/// One Restart Manager session over `files`.
fn query(files: &[Vec<u16>]) -> Result<Vec<RM_PROCESS_INFO>, WIN32_ERROR> {
    let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
    let mut handle = 0u32;
    let r = unsafe { RmStartSession(&mut handle, None, PWSTR(key.as_mut_ptr())) };
    if r != ERROR_SUCCESS {
        return Err(r);
    }
    let session = Session(handle);

    for chunk in files.chunks(REGISTER_BATCH) {
        let names: Vec<PCWSTR> = chunk.iter().map(|w| PCWSTR(w.as_ptr())).collect();
        let r = unsafe { RmRegisterResources(session.0, Some(&names), None, None) };
        if r != ERROR_SUCCESS {
            return Err(r);
        }
    }

    let mut infos: Vec<RM_PROCESS_INFO> = Vec::new();
    for _ in 0..5 {
        let (mut needed, mut count, mut reasons) = (0u32, infos.len() as u32, 0u32);
        let r = unsafe {
            RmGetList(
                session.0,
                &mut needed,
                &mut count,
                (!infos.is_empty()).then_some(infos.as_mut_ptr()),
                &mut reasons,
            )
        };
        if r == ERROR_SUCCESS {
            infos.truncate(count as usize);
            return Ok(infos);
        }
        if r != ERROR_MORE_DATA {
            return Err(r);
        }
        // The list can grow between calls; leave room.
        infos = vec![unsafe { std::mem::zeroed() }; needed as usize + 4];
    }
    Err(ERROR_MORE_DATA)
}

/// Queries `files`, splitting the set in half whenever the Restart Manager
/// rejects it. It fails a whole session when any registered file is in use by
/// a process it cannot inspect (for example a system DLL loaded by protected
/// processes), so one such file must not hide the answer for all the others.
fn query_split(files: &[Vec<u16>], out: &mut Vec<RM_PROCESS_INFO>, unchecked: &mut usize) {
    if files.is_empty() {
        return;
    }
    match query(files) {
        Ok(infos) => out.extend(infos),
        Err(_) if files.len() == 1 => *unchecked += 1,
        Err(_) => {
            let (a, b) = files.split_at(files.len() / 2);
            query_split(a, out, unchecked);
            query_split(b, out, unchecked);
        }
    }
}

/// Collects files under `dir` (not following links). Returns true when capped.
fn walk(dir: &Path, files: &mut Vec<PathBuf>) -> bool {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in read.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                stack.push(entry.path());
            } else {
                files.push(entry.path());
                if files.len() >= MAX_FILES {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handles::{HandleLocker, HandleReport, HandleSummary};

    fn handle_report() -> HandleReport {
        HandleReport {
            lockers: vec![HandleLocker {
                pid: 10,
                create_time: 20,
                name: "test.exe".into(),
                path: r"C:\test".into(),
                count: 2,
            }],
            summary: HandleSummary {
                available: true,
                ..Default::default()
            },
        }
    }

    #[test]
    fn merges_the_same_instance_but_does_not_merge_reused_pids() {
        let base = LockReport {
            lockers: vec![Locker {
                pid: 10,
                create_time: 20,
                app_name: "Friendly name".into(),
                service: Some("service".into()),
                kind: "service",
                handle_count: 0,
                handle_path: None,
            }],
            files_checked: 1,
            unchecked: 0,
            truncated: false,
            handles: None,
            restart_error: None,
        };
        let mut result = LockReport::combine(Ok(base), handle_report()).unwrap();
        assert_eq!(result.lockers.len(), 1);
        assert_eq!(result.lockers[0].handle_count, 2);
        assert_eq!(result.lockers[0].app_name, "Friendly name");
        let mut reused = handle_report();
        reused.lockers[0].create_time = 30;
        result.merge_handles(reused);
        assert_eq!(result.lockers.len(), 2);
    }

    #[test]
    fn privileged_results_survive_a_restart_manager_access_failure() {
        let report = LockReport::combine(Err(ActionError::AccessDenied), handle_report()).unwrap();
        assert_eq!(report.lockers.len(), 1);
        assert!(report.restart_error.is_some());
        assert!(LockReport::combine(Err(ActionError::NotFound), HandleReport::default()).is_err());
    }
}
