//! Logon sessions and the users signed in to them.
//!
//! The local session manager answers the enumeration for every session
//! without elevation, unlike the tokens of other users' processes. A process
//! belongs to the user of its session (`ProcessRow::session_id`); session 0
//! and sessions at the sign-in screen have no user and are not listed.

use std::ffi::c_void;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogonSession {
    pub id: u32,
    /// The account name of the signed-in user.
    pub user: Arc<str>,
    /// The account's domain; the computer's name for a local account.
    pub domain: Arc<str>,
    /// The session this process runs in.
    pub current: bool,
    /// Signed in with no display or remote client attached.
    pub disconnected: bool,
}

/// `WTS_SESSION_INFO_1W`.
#[repr(C)]
struct SessionInfo {
    exec_env_id: u32,
    state: i32,
    session_id: u32,
    session_name: *const u16,
    host_name: *const u16,
    user_name: *const u16,
    domain_name: *const u16,
    farm_name: *const u16,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<SessionInfo>() == 56);

/// `WTSDisconnected` of `WTS_CONNECTSTATE_CLASS`.
const STATE_DISCONNECTED: i32 = 4;
/// `WTSTypeSessionInfoLevel1` of `WTS_TYPE_CLASS`.
const TYPE_SESSION_INFO_LEVEL_1: u32 = 2;

#[link(name = "wtsapi32")]
unsafe extern "system" {
    fn WTSEnumerateSessionsExW(
        server: *mut c_void,
        level: *mut u32,
        filter: u32,
        sessions: *mut *mut SessionInfo,
        count: *mut u32,
    ) -> i32;
    fn WTSFreeMemoryExW(class: u32, memory: *mut c_void, count: u32) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcessId() -> u32;
    fn ProcessIdToSessionId(process: u32, session: *mut u32) -> i32;
}

/// # Safety
/// `text` is null or points to a NUL-terminated UTF-16 string.
unsafe fn wide(text: *const u16) -> String {
    if text.is_null() {
        return String::new();
    }
    let mut length = 0;
    while unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, length) })
}

/// The session this process runs in.
fn current_session() -> Option<u32> {
    let mut session = 0;
    (unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) } != 0).then_some(session)
}

/// The sessions a user is signed in to, in session order. `None` when the
/// session manager does not answer: the previous list then stays in use.
fn enumerate() -> Option<Vec<(u32, String, String, bool)>> {
    let mut level = 1;
    let mut list = std::ptr::null_mut();
    let mut count = 0;
    // The current server is a null handle.
    if unsafe {
        WTSEnumerateSessionsExW(std::ptr::null_mut(), &mut level, 0, &mut list, &mut count)
    } == 0
        || list.is_null()
    {
        return None;
    }
    let mut sessions: Vec<_> = unsafe { std::slice::from_raw_parts(list, count as usize) }
        .iter()
        .filter_map(|session| {
            let user = unsafe { wide(session.user_name) };
            (!user.is_empty()).then(|| {
                (
                    session.session_id,
                    user,
                    unsafe { wide(session.domain_name) },
                    session.state == STATE_DISCONNECTED,
                )
            })
        })
        .collect();
    unsafe { WTSFreeMemoryExW(TYPE_SESSION_INFO_LEVEL_1, list.cast(), count) };
    sessions.sort_by_key(|session| session.0);
    Some(sessions)
}

/// Users sign in and out rarely, and the session manager takes about 0.3 ms
/// (at times a few) to answer a call made after a pause: every other sample
/// at the default interval is often enough.
const REFRESH: Duration = Duration::from_millis(1900);

pub(crate) struct SessionTracker {
    current: Option<u32>,
    sessions: Vec<LogonSession>,
    read: Option<Instant>,
}

impl SessionTracker {
    pub fn new() -> Self {
        Self {
            current: current_session(),
            sessions: Vec::new(),
            read: None,
        }
    }

    /// The signed-in sessions, read again once `REFRESH` has passed. Names
    /// are shared with the previous sample's: nothing is allocated while
    /// nobody signs in or out.
    pub fn sample(&mut self) -> Vec<LogonSession> {
        let now = Instant::now();
        if self
            .read
            .is_some_and(|read| now.saturating_duration_since(read) < REFRESH)
        {
            return self.sessions.clone();
        }
        self.read = Some(now);
        if let Some(found) = enumerate() {
            let unchanged = found.len() == self.sessions.len()
                && found.iter().zip(&self.sessions).all(|(new, old)| {
                    new.0 == old.id
                        && *new.1 == *old.user
                        && *new.2 == *old.domain
                        && new.3 == old.disconnected
                });
            if !unchanged {
                self.sessions = found
                    .into_iter()
                    .map(|(id, user, domain, disconnected)| LogonSession {
                        id,
                        user: user.into(),
                        domain: domain.into(),
                        current: self.current == Some(id),
                        disconnected,
                    })
                    .collect();
            }
        }
        self.sessions.clone()
    }
}

/// The signed-in sessions, read once (not part of sampling).
pub fn list() -> Vec<LogonSession> {
    SessionTracker::new().sample()
}

#[cfg(test)]
mod tests {
    use super::{SessionTracker, current_session};

    #[test]
    fn the_own_session_is_listed_once_and_marked_current() {
        let mut tracker = SessionTracker::new();
        let sessions = tracker.sample();
        // A test run by a service account has no signed-in session of its own.
        let Some(own) = current_session().filter(|id| *id != 0) else {
            return;
        };
        let current: Vec<_> = sessions.iter().filter(|session| session.current).collect();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].id, own);
        assert!(!current[0].user.is_empty());
        assert!(sessions.windows(2).all(|pair| pair[0].id < pair[1].id));
        // An unchanged list shares its names with the previous sample.
        let again = tracker.sample();
        assert_eq!(again, sessions);
        assert!(std::sync::Arc::ptr_eq(&again[0].user, &sessions[0].user));
    }
}
