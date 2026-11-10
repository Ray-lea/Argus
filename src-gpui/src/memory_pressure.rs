//! Keep optional rendering allocations inside the current process commit budget.
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

pub(crate) fn available_memory() -> usize {
    use windows::Win32::System::SystemInformation::{
        GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
    };
    static CHECKED: AtomicU64 = AtomicU64::new(0);
    static AVAILABLE: AtomicUsize = AtomicUsize::new(usize::MAX);
    let now = unsafe { GetTickCount64() };
    let previous = CHECKED.load(Ordering::Relaxed);
    if now.saturating_sub(previous) >= 100
        && CHECKED
            .compare_exchange(previous, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    {
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        if unsafe { GlobalMemoryStatusEx(&mut status) }.is_ok() {
            AVAILABLE.store(
                job_headroom().min(
                    status
                        .ullAvailPhys
                        .min(status.ullAvailPageFile)
                        .min(status.ullAvailVirtual) as usize,
                ),
                Ordering::Relaxed,
            );
        }
    }
    AVAILABLE.load(Ordering::Relaxed)
}

fn job_headroom() -> usize {
    use windows::Win32::System::{
        JobObjects::*,
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
        Threading::GetCurrentProcess,
    };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    if unsafe {
        QueryInformationJobObject(
            None,
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
            None,
        )
    }
    .is_err()
    {
        return usize::MAX;
    }
    let flags = limits.BasicLimitInformation.LimitFlags;
    if !flags.contains(JOB_OBJECT_LIMIT_JOB_MEMORY)
        && !flags.contains(JOB_OBJECT_LIMIT_PROCESS_MEMORY)
    {
        return usize::MAX;
    }
    let mut memory = PROCESS_MEMORY_COUNTERS_EX::default();
    if unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            std::mem::size_of_val(&memory) as u32,
        )
    }
    .is_err()
    {
        return 0;
    }
    let mut available = usize::MAX;
    if flags.contains(JOB_OBJECT_LIMIT_PROCESS_MEMORY) {
        available = limits
            .ProcessMemoryLimit
            .saturating_sub(memory.PrivateUsage);
    }
    if flags.contains(JOB_OBJECT_LIMIT_JOB_MEMORY) {
        let used = job_usage().unwrap_or(limits.PeakJobMemoryUsed);
        available = available.min(limits.JobMemoryLimit.saturating_sub(used));
    }
    available
}

fn job_usage() -> Option<usize> {
    use windows::Win32::{
        Foundation::CloseHandle,
        System::{
            JobObjects::*,
            ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
            Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    };
    #[repr(C)]
    struct Members {
        assigned: u32,
        count: u32,
        pids: [usize; 64],
    }
    let mut members = Members {
        assigned: 0,
        count: 0,
        pids: [0; 64],
    };
    unsafe {
        QueryInformationJobObject(
            None,
            JobObjectBasicProcessIdList,
            (&mut members as *mut Members).cast(),
            std::mem::size_of_val(&members) as u32,
            None,
        )
    }
    .ok()?;
    let mut total = 0usize;
    for pid in members.pids.get(..members.count as usize)? {
        let process =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, *pid as u32) }.ok()?;
        let mut memory = PROCESS_MEMORY_COUNTERS_EX::default();
        let result = unsafe {
            GetProcessMemoryInfo(
                process,
                (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
                std::mem::size_of_val(&memory) as u32,
            )
        };
        unsafe {
            let _ = CloseHandle(process);
        }
        result.ok()?;
        total = total.saturating_add(memory.PrivateUsage);
    }
    Some(total)
}

/// Decommits unused LFH caches, then trims the resident working set once.
/// Allocations still in use remain committed and return as they are touched.
pub(crate) fn release_process_memory() {
    use windows::Win32::System::{
        Memory::{HeapOptimizeResources, HeapSetInformation},
        SystemServices::{
            HEAP_OPTIMIZE_RESOURCES_CURRENT_VERSION, HEAP_OPTIMIZE_RESOURCES_INFORMATION,
        },
        Threading::{GetCurrentProcess, SetProcessWorkingSetSize},
    };
    let info = HEAP_OPTIMIZE_RESOURCES_INFORMATION {
        Version: HEAP_OPTIMIZE_RESOURCES_CURRENT_VERSION,
        Flags: 0,
    };
    unsafe {
        if let Err(error) = HeapSetInformation(
            None,
            HeapOptimizeResources,
            Some((&raw const info).cast()),
            size_of_val(&info),
        ) {
            argus_core::diagnostics::windows_failure("hidden heap optimization", &error);
        }
        if let Err(error) = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX) {
            argus_core::diagnostics::windows_failure("hidden working set trim", &error);
        }
    }
}

pub(crate) fn reduced(headroom: usize, was_reduced: bool) -> bool {
    headroom < if was_reduced { 64 << 20 } else { 32 << 20 }
}

#[cfg(test)]
mod tests {
    use super::reduced;

    #[test]
    fn recovery_keeps_a_margin_to_avoid_rebuilding_under_pressure() {
        assert!(reduced(16 << 20, false));
        assert!(reduced(48 << 20, true));
        assert!(!reduced(48 << 20, false));
        assert!(!reduced(64 << 20, true));
    }

    #[test]
    fn the_current_job_limit_restricts_optional_allocations() {
        use std::os::windows::process::CommandExt;
        use windows::Win32::{
            Foundation::CloseHandle,
            System::{
                JobObjects::*,
                ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
                Threading::{CREATE_NO_WINDOW, GetCurrentProcess},
            },
        };
        const CHILD: &str = "ARGUS_TEST_MEMORY_JOB_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "memory_pressure::tests::the_current_job_limit_restricts_optional_allocations",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .creation_flags(CREATE_NO_WINDOW.0)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        unsafe {
            let job = CreateJobObjectW(None, None).unwrap();
            let mut memory = PROCESS_MEMORY_COUNTERS_EX::default();
            GetProcessMemoryInfo(
                GetCurrentProcess(),
                (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
                std::mem::size_of_val(&memory) as u32,
            )
            .unwrap();
            let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                    LimitFlags: JOB_OBJECT_LIMIT_JOB_MEMORY,
                    ..Default::default()
                },
                JobMemoryLimit: memory.PrivateUsage + (16 << 20),
                ..Default::default()
            };
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
            .unwrap();
            AssignProcessToJobObject(job, GetCurrentProcess()).unwrap();
            let available = super::job_headroom();
            CloseHandle(job).unwrap();
            assert!(
                available <= 16 << 20 && available > 8 << 20,
                "headroom={available}"
            );
        }
    }
}
