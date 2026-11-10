//! Cached storage handles cooperate with PnP removal without a window thread.
use std::ffi::c_void;
use std::sync::{Condvar, Mutex};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_NOTIFY_ACTION, CM_NOTIFY_ACTION_DEVICEQUERYREMOVE, CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED,
    CM_NOTIFY_ACTION_DEVICEREMOVECOMPLETE, CM_NOTIFY_ACTION_DEVICEREMOVEPENDING,
    CM_NOTIFY_EVENT_DATA, CM_NOTIFY_FILTER, CM_NOTIFY_FILTER_TYPE_DEVICEHANDLE,
    CM_Register_Notification, CM_Unregister_Notification, CONFIGRET, CR_SUCCESS, HCMNOTIFICATION,
};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows::core::PCWSTR;

use crate::util::{OwnedHandle, wide};

const DISCONNECTED: u32 = 1167;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Active,
    QueryRemove,
    Removed,
}

struct State {
    handle: Option<OwnedHandle>,
    phase: Phase,
    generation: u64,
    opening: bool,
}

struct Context(Mutex<State>, Condvar);

impl Context {
    fn event(&self, action: CM_NOTIFY_ACTION) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        match action {
            CM_NOTIFY_ACTION_DEVICEQUERYREMOVE if state.phase != Phase::Removed => {
                state.phase = Phase::QueryRemove;
                state.generation = state.generation.wrapping_add(1);
                // Finish any in-flight read, then close before accepting removal.
                state.handle.take();
            }
            CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED if state.phase != Phase::Removed => {
                state.phase = Phase::Active;
                state.generation = state.generation.wrapping_add(1);
                // Reopening is deferred to the sampler. The existing PnP
                // registration remains valid after its original handle closes.
            }
            CM_NOTIFY_ACTION_DEVICEREMOVEPENDING | CM_NOTIFY_ACTION_DEVICEREMOVECOMPLETE => {
                state.phase = Phase::Removed;
                state.generation = state.generation.wrapping_add(1);
                state.handle.take();
            }
            _ => {}
        }
        // A reopen already in flight must finish (and discard its new handle)
        // before this callback acknowledges removal to Windows.
        while state.phase != Phase::Active && state.opening {
            state = self
                .1
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
    }
}

pub(super) struct DeviceHandle {
    path: String,
    context: Option<Box<Context>>,
    notification: Option<HCMNOTIFICATION>,
    cached: bool,
}

// The callback owns no references to DeviceHandle itself. Its stable boxed
// context synchronizes handle access, and unregister waits for callbacks.
unsafe impl Send for DeviceHandle {}

impl DeviceHandle {
    pub(super) fn open(path: &str) -> Result<Self, u32> {
        let handle = open_raw(path)?;
        let target = handle.0;
        let context = Box::new(Context(
            Mutex::new(State {
                handle: Some(handle),
                phase: Phase::Active,
                generation: 0,
                opening: false,
            }),
            Condvar::new(),
        ));
        let mut filter = CM_NOTIFY_FILTER {
            cbSize: size_of::<CM_NOTIFY_FILTER>() as u32,
            FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEHANDLE,
            ..Default::default()
        };
        filter.u.DeviceHandle.hTarget = target;
        let mut notification = HCMNOTIFICATION::default();
        let result = unsafe {
            CM_Register_Notification(
                &filter,
                Some((&*context as *const Context).cast()),
                Some(notified),
                &mut notification,
            )
        };
        Ok(Self::finish_registration(
            path,
            context,
            result,
            notification,
        ))
    }

    fn finish_registration(
        path: &str,
        context: Box<Context>,
        result: CONFIGRET,
        notification: HCMNOTIFICATION,
    ) -> Self {
        let cached = result == CR_SUCCESS;
        if !cached {
            // Unsupported notifications must never leave a permanently open
            // device. Preserve reads using a query-scoped handle instead.
            context.0.lock().unwrap().handle.take();
            crate::diagnostics::warn(
                "disk.removal.register",
                format!("config_ret=0x{:08X}", result.0),
            );
        }
        Self {
            path: path.into(),
            context: Some(context),
            notification: cached.then_some(notification),
            cached,
        }
    }

    pub(super) fn read<T>(
        &self,
        read: impl FnOnce(HANDLE, u64) -> Result<T, u32>,
    ) -> Result<T, u32> {
        self.read_with_open(read, open_raw)
    }

    fn read_with_open<T>(
        &self,
        read: impl FnOnce(HANDLE, u64) -> Result<T, u32>,
        open: impl FnOnce(&str) -> Result<OwnedHandle, u32>,
    ) -> Result<T, u32> {
        if !self.cached {
            let handle = open(&self.path)?;
            return read(handle.0, 0);
        }
        let context = self.context.as_ref().unwrap();
        let mut state = context.0.lock().unwrap_or_else(|error| error.into_inner());
        if state.phase != Phase::Active {
            return Err(DISCONNECTED);
        }
        if state.handle.is_none() {
            if state.opening {
                return Err(21);
            }
            let generation = state.generation;
            state.opening = true;
            drop(state);
            let handle = open(&self.path);
            state = context.0.lock().unwrap_or_else(|error| error.into_inner());
            state.opening = false;
            context.1.notify_all();
            if state.phase != Phase::Active || state.generation != generation {
                drop(handle);
                return Err(DISCONNECTED);
            }
            state.handle = Some(handle?);
        }
        read(state.handle.as_ref().unwrap().0, state.generation)
    }

    pub(super) fn suspended(&self) -> bool {
        self.context
            .as_ref()
            .unwrap()
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .phase
            == Phase::QueryRemove
    }

    pub(super) fn removed(&self) -> bool {
        self.context
            .as_ref()
            .unwrap()
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .phase
            == Phase::Removed
    }
}

impl Drop for DeviceHandle {
    fn drop(&mut self) {
        if let Some(context) = &self.context {
            context.event(CM_NOTIFY_ACTION_DEVICEREMOVECOMPLETE);
        }
        if let Some(notification) = self.notification.take() {
            // This runs on the owning thread, never inside the PnP callback,
            // and without holding its state lock.
            let result = unsafe { CM_Unregister_Notification(notification) };
            if result != CR_SUCCESS {
                crate::diagnostics::warn(
                    "disk.removal.unregister",
                    format!("config_ret=0x{:08X}", result.0),
                );
                // An unexpected unregister failure cannot free a context
                // that the OS may still call. Its device handle is closed.
                Box::leak(self.context.take().unwrap());
            }
        }
    }
}

unsafe extern "system" fn notified(
    _: HCMNOTIFICATION,
    context: *const c_void,
    action: CM_NOTIFY_ACTION,
    _: *const CM_NOTIFY_EVENT_DATA,
    _: u32,
) -> u32 {
    if let Some(context) = unsafe { context.cast::<Context>().as_ref() } {
        context.event(action);
    }
    0
}

fn open_raw(path: &str) -> Result<OwnedHandle, u32> {
    unsafe {
        CreateFileW(
            PCWSTR(wide(path).as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .map(OwnedHandle)
    .map_err(super::native_code)
}

/// A volume query made inside its physical device's registered read guard.
/// Close this child handle before that guard can acknowledge device removal.
pub(super) fn read_scoped<T>(
    path: &str,
    read: impl FnOnce(HANDLE) -> Result<T, u32>,
) -> Result<T, u32> {
    let handle = open_raw(path)?;
    read(handle.0)
}

#[cfg(test)]
impl DeviceHandle {
    pub(super) fn registered(&self) -> bool {
        self.notification.is_some()
    }

    pub(super) fn fixture(path: &std::path::Path) -> Self {
        use std::os::windows::{fs::OpenOptionsExt, io::IntoRawHandle};
        let file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
            .open(path)
            .unwrap();
        Self {
            path: path.to_string_lossy().into_owned(),
            context: Some(Box::new(Context(
                Mutex::new(State {
                    handle: Some(OwnedHandle(HANDLE(file.into_raw_handle()))),
                    phase: Phase::Active,
                    generation: 0,
                    opening: false,
                }),
                Condvar::new(),
            ))),
            notification: None,
            cached: true,
        }
    }

    pub(super) fn notify(&self, action: CM_NOTIFY_ACTION) {
        let context = &**self.context.as_ref().unwrap() as *const Context;
        assert_eq!(
            unsafe {
                notified(
                    HCMNOTIFICATION::default(),
                    context.cast(),
                    action,
                    std::ptr::null(),
                    0,
                )
            },
            0
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::mpsc, time::Duration};

    #[test]
    fn query_remove_closes_the_handle_and_cancellation_reopens_it() {
        let file = super::super::tests::FileFixture::new();
        let handle = DeviceHandle::fixture(&file.path);
        let moved = file.path.with_extension("moved");
        assert!(fs::rename(&file.path, &moved).is_err());
        assert_eq!(handle.read(|_, generation| Ok(generation)), Ok(0));
        handle.notify(CM_NOTIFY_ACTION_DEVICEQUERYREMOVE);
        fs::rename(&file.path, &moved).unwrap();
        fs::rename(&moved, &file.path).unwrap();
        assert_eq!(
            handle.read(|_, _| panic!("I/O during query removal")),
            Err::<(), _>(DISCONNECTED)
        );
        handle.notify(CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED);
        assert_eq!(handle.read(|_, generation| Ok(generation)), Ok(2));
        handle.notify(CM_NOTIFY_ACTION_DEVICEREMOVEPENDING);
        handle.notify(CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED);
        assert!(handle.removed());
        assert_eq!(handle.read(|_, _| Ok(())), Err(DISCONNECTED));
    }

    #[test]
    fn surprise_removal_closes_without_a_query_and_never_reopens() {
        let file = super::super::tests::FileFixture::new();
        let handle = DeviceHandle::fixture(&file.path);
        handle.notify(CM_NOTIFY_ACTION_DEVICEREMOVECOMPLETE);
        fs::remove_file(&file.path).unwrap();
        assert!(handle.removed());
        assert_eq!(handle.read(|_, _| Ok(())), Err(DISCONNECTED));
    }

    #[test]
    fn failed_registration_uses_query_scoped_handles() {
        let file = super::super::tests::FileFixture::new();
        let mut original = DeviceHandle::fixture(&file.path);
        let context = original.context.take().unwrap();
        // Transfer the fixture context into the same registration failure path.
        drop(original);
        let handle = DeviceHandle::finish_registration(
            &file.path.to_string_lossy(),
            context,
            CONFIGRET(0x13),
            HCMNOTIFICATION::default(),
        );
        handle.read(|_, _| Ok(())).unwrap();
        fs::remove_file(&file.path).unwrap();
        assert_eq!(handle.read(|_, _| Ok(())), Err(2));
    }

    #[test]
    fn removal_waits_for_the_current_read_before_acknowledging() {
        let file = super::super::tests::FileFixture::new();
        let handle = DeviceHandle::fixture(&file.path);
        let context = &**handle.context.as_ref().unwrap();
        std::thread::scope(|scope| {
            let (started, starting) = mpsc::channel();
            let (finished, completion) = mpsc::channel();
            handle
                .read(|_, _| {
                    scope.spawn(move || {
                        started.send(()).unwrap();
                        context.event(CM_NOTIFY_ACTION_DEVICEQUERYREMOVE);
                        finished.send(()).unwrap();
                    });
                    starting.recv_timeout(Duration::from_secs(2)).unwrap();
                    assert!(completion.try_recv().is_err());
                    Ok(())
                })
                .unwrap();
            completion.recv_timeout(Duration::from_secs(2)).unwrap();
        });
        assert!(handle.suspended());
        fs::remove_file(&file.path).unwrap();
    }

    #[test]
    fn a_reopen_racing_removal_is_closed_before_the_callback_returns() {
        let file = super::super::tests::FileFixture::new();
        let handle = DeviceHandle::fixture(&file.path);
        handle.notify(CM_NOTIFY_ACTION_DEVICEQUERYREMOVE);
        handle.notify(CM_NOTIFY_ACTION_DEVICEQUERYREMOVEFAILED);
        let context = &**handle.context.as_ref().unwrap();
        std::thread::scope(|scope| {
            let (finished, completion) = mpsc::channel();
            let mut requested = false;
            let mut acknowledged_early = false;
            let mut queried = false;
            let result: Result<(), _> = handle.read_with_open(
                |_, _| {
                    queried = true;
                    Ok(())
                },
                |path| {
                    let opened = open_raw(path)?;
                    scope.spawn(move || {
                        context.event(CM_NOTIFY_ACTION_DEVICEQUERYREMOVE);
                        finished.send(()).unwrap();
                    });
                    let deadline = std::time::Instant::now() + Duration::from_secs(2);
                    while std::time::Instant::now() < deadline {
                        if context.0.lock().unwrap().phase == Phase::QueryRemove {
                            requested = true;
                            break;
                        }
                        std::thread::yield_now();
                    }
                    acknowledged_early = completion.try_recv().is_ok();
                    Ok(opened)
                },
            );
            assert!(requested && !acknowledged_early && !queried);
            assert_eq!(result, Err(DISCONNECTED));
            completion.recv_timeout(Duration::from_secs(2)).unwrap();
        });
        assert!(context.0.lock().unwrap().handle.is_none());
    }
}
