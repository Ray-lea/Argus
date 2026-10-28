//! In-place window fades, as the Tauri host had them: showing, hiding to the
//! tray, minimizing and restoring fade the whole window instead of the system's
//! zoom animations. Native visibility still drives sampling; the temporary
//! layered style exists only while a fade is running.

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{
    DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, LWA_ALPHA,
    PostMessageW, SC_MINIMIZE, SC_RESTORE, STYLESTRUCT, SW_HIDE, SW_MINIMIZE, SW_RESTORE,
    SW_SHOWNOACTIVATE, SendMessageW, SetLayeredWindowAttributes, SetWindowLongPtrW, ShowWindow,
    WM_APP, WM_NCDESTROY, WM_STYLECHANGED, WM_STYLECHANGING, WM_SYSCOMMAND, WM_WINDOWPOSCHANGED,
    WS_EX_LAYERED,
};

const SUBCLASS: usize = 0x46414445;
const FRAME: u32 = WM_APP + 0x46;
/// A request from another thread, carried out on the window's own.
const REQUEST: u32 = WM_APP + 0x47;
static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
static LEAVING: AtomicBool = AtomicBool::new(false);
static CHANGING: AtomicUsize = AtomicUsize::new(0);
/// The interface's motion preference, kept for a window installed after it
/// was sent (the interface resolves it while the window is being created).
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether a visibility change is under way; position notifications in the
/// middle of one are not the final state.
pub fn changing_visibility() -> bool {
    CHANGING.load(Ordering::Relaxed) != 0
}

struct VisibilityChange;

impl Drop for VisibilityChange {
    fn drop(&mut self) {
        CHANGING.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Whether the window is fading out (to the tray or the taskbar).
pub fn is_leaving() -> bool {
    LEAVING.load(Ordering::Relaxed)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Shown,
    Hidden,
    Minimized,
}

#[derive(Clone, Copy)]
struct Fade {
    start: Option<Instant>,
    from: u8,
    target: Target,
    duration: Duration,
}

impl Fade {
    fn new(from: u8, target: Target) -> Self {
        let distance = if target == Target::Shown {
            255 - from
        } else {
            from
        };
        let full_ms = if target == Target::Shown {
            180.0
        } else {
            140.0
        };
        Self {
            start: None,
            from,
            target,
            duration: Duration::from_secs_f64(full_ms * f64::from(distance) / 255_000.0),
        }
    }

    fn sample(&mut self, now: Instant) -> (u8, bool) {
        // Showing and focusing can block the window thread. Start with the
        // first frame it can present, not before that synchronous work.
        let elapsed = now.saturating_duration_since(*self.start.get_or_insert(now));
        if elapsed >= self.duration {
            return (if self.target == Target::Shown { 255 } else { 0 }, true);
        }
        let progress = elapsed.as_secs_f64() / self.duration.as_secs_f64();
        let opacity = if self.target == Target::Shown {
            // A quick response followed by a soft arrival.
            f64::from(self.from) + f64::from(255 - self.from) * (1.0 - (1.0 - progress).powi(3))
        } else {
            f64::from(self.from) * (1.0 - progress * progress)
        };
        (opacity.round() as u8, false)
    }
}

#[derive(Default)]
struct Ticker {
    stopped: AtomicBool,
    pending: AtomicBool,
}

struct Motion {
    id: usize,
    enabled: Cell<bool>,
    fade: Cell<Option<Fade>>,
    layered: Cell<bool>,
    opacity: Cell<u8>,
    shown: Cell<bool>,
    ticker: RefCell<Option<Arc<Ticker>>>,
}

impl Motion {
    fn change(&self, hwnd: HWND, target: Target) {
        CHANGING.fetch_add(1, Ordering::Relaxed);
        let guard = VisibilityChange;
        apply(hwnd, target);
        drop(guard);
        if !changing_visibility() {
            self.shown.set(shown(hwnd));
            crate::shell::resync_visibility(hwnd);
        }
    }

    fn alpha(&self, hwnd: HWND, alpha: u8) -> bool {
        unsafe {
            if !self.layered.get() {
                let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                // Do not take ownership of transparency supplied by another component.
                if style & WS_EX_LAYERED.0 as isize != 0 {
                    return false;
                }
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED.0 as isize);
                self.layered.set(true);
            }
            self.opacity.set(alpha);
            SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA).is_ok()
        }
    }

    fn opaque(&self, hwnd: HWND) {
        if self.layered.replace(false) {
            unsafe {
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
                let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style & !(WS_EX_LAYERED.0 as isize));
            }
        }
    }

    fn stop_clock(&self) {
        if let Some(ticker) = self.ticker.borrow_mut().take() {
            ticker.stopped.store(true, Ordering::Release);
        }
    }

    fn finish(&self, hwnd: HWND, target: Target) {
        self.fade.set(None);
        LEAVING.store(false, Ordering::Relaxed);
        self.stop_clock();
        // Disappear before removing transparency: there must be no opaque last frame.
        if target != Target::Shown {
            self.change(hwnd, target);
        }
        self.opaque(hwnd);
    }

    fn start_clock(&self, hwnd: HWND) -> bool {
        if self.ticker.borrow().is_some() {
            return true;
        }
        let ticker = Arc::new(Ticker::default());
        let clock = ticker.clone();
        let handle = hwnd.0 as usize;
        let id = self.id;
        let spawned = std::thread::Builder::new()
            .name("window-fade".into())
            .spawn(move || {
                while !clock.stopped.load(Ordering::Acquire) {
                    // Rust uses a high-resolution waitable timer on supported
                    // Windows versions, without changing system timer resolution.
                    // DwmFlush can halve the cadence of a maximized layered window.
                    std::thread::sleep(Duration::from_millis(4));
                    if clock.stopped.load(Ordering::Acquire) {
                        break;
                    }
                    if !clock.pending.swap(true, Ordering::AcqRel)
                        && unsafe {
                            PostMessageW(Some(HWND(handle as _)), FRAME, WPARAM(id), LPARAM(0))
                        }
                        .is_err()
                    {
                        break;
                    }
                }
            });
        if spawned.is_err() {
            return false;
        }
        *self.ticker.borrow_mut() = Some(ticker);
        true
    }

    fn request(&self, hwnd: HWND, target: Target) {
        let previous = self.fade.get();
        if previous.is_some_and(|fade| fade.target == target) {
            return;
        }
        let shown = shown(hwnd);
        if !self.enabled.get() || (!shown && target != Target::Shown) {
            self.finish(hwnd, target);
            if target == Target::Shown {
                self.change(hwnd, target);
            }
            return;
        }
        if previous.is_none() && shown && target == Target::Shown {
            return;
        }
        let from = if previous.is_some() {
            self.opacity.get()
        } else if shown {
            255
        } else {
            0
        };
        if !self.alpha(hwnd, from) {
            self.finish(hwnd, target);
            self.change(hwnd, target);
            return;
        }
        self.fade.set(Some(Fade::new(from, target)));
        LEAVING.store(target != Target::Shown, Ordering::Relaxed);
        if target == Target::Shown && !shown {
            self.change(hwnd, target);
        }
        if !self.start_clock(hwnd) {
            self.finish(hwnd, target);
        }
    }
}

fn shown(hwnd: HWND) -> bool {
    unsafe { IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() }
}

/// The visibility change itself. Showing does not activate: callers that
/// should take the foreground focus the window afterwards (`shell::show_main`).
fn apply(hwnd: HWND, target: Target) {
    unsafe {
        let command = match target {
            Target::Shown if IsIconic(hwnd).as_bool() => SW_RESTORE,
            Target::Shown => SW_SHOWNOACTIVATE,
            Target::Hidden => SW_HIDE,
            Target::Minimized => SW_MINIMIZE,
        };
        let _ = ShowWindow(hwnd, command);
    }
}

/// Installed on the window's thread before it can be minimized or restored.
pub fn install(hwnd: HWND) {
    unsafe {
        // Per-window only: no system setting changes or taskbar zoom animations.
        let disabled = 1i32;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            &disabled as *const _ as _,
            size_of_val(&disabled) as u32,
        );
        let motion = Box::into_raw(Box::new(Motion {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            // The interface sends the resolved motion preference on startup.
            enabled: Cell::new(ENABLED.load(Ordering::Relaxed)),
            fade: Cell::new(None),
            layered: Cell::new(false),
            opacity: Cell::new(255),
            shown: Cell::new(shown(hwnd)),
            ticker: RefCell::new(None),
        }));
        if !SetWindowSubclass(hwnd, Some(procedure), SUBCLASS, motion as usize).as_bool() {
            drop(Box::from_raw(motion));
        }
    }
}

#[derive(Clone, Copy)]
enum Op {
    Go(Target),
    Enable(bool),
}

impl Op {
    fn encode(self) -> (usize, isize) {
        match self {
            Op::Go(Target::Shown) => (1, 0),
            Op::Go(Target::Hidden) => (2, 0),
            Op::Go(Target::Minimized) => (3, 0),
            Op::Enable(enabled) => (4, isize::from(enabled)),
        }
    }

    fn decode(w: WPARAM, l: LPARAM) -> Option<Self> {
        Some(match w.0 {
            1 => Op::Go(Target::Shown),
            2 => Op::Go(Target::Hidden),
            3 => Op::Go(Target::Minimized),
            4 => Op::Enable(l.0 != 0),
            _ => return None,
        })
    }
}

/// Carries out `op` on the window's thread: the fade state lives there.
fn dispatch(hwnd: HWND, op: Op) {
    let own = unsafe { GetWindowThreadProcessId(hwnd, None) == GetCurrentThreadId() };
    if own {
        perform(hwnd, op);
    } else {
        let (w, l) = op.encode();
        unsafe {
            SendMessageW(hwnd, REQUEST, Some(WPARAM(w)), Some(LPARAM(l)));
        }
    }
}

fn perform(hwnd: HWND, op: Op) {
    let mut data = 0;
    let found =
        unsafe { GetWindowSubclass(hwnd, Some(procedure), SUBCLASS, Some(&mut data)) }.as_bool();
    let motion = found.then(|| unsafe { &*(data as *const Motion) });
    match (motion, op) {
        (Some(motion), Op::Enable(enabled)) => {
            motion.enabled.set(enabled);
            if !enabled && let Some(fade) = motion.fade.get() {
                motion.finish(hwnd, fade.target);
            }
        }
        (Some(motion), Op::Go(target)) => motion.request(hwnd, target),
        (None, Op::Go(target)) => apply(hwnd, target),
        (None, Op::Enable(_)) => {}
    }
}

/// Turns the fades on or off with the interface's motion preference; a window
/// installed later starts with the last preference.
pub fn set_enabled(hwnd: Option<HWND>, enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    if let Some(hwnd) = hwnd {
        dispatch(hwnd, Op::Enable(enabled));
    }
}

pub fn show(hwnd: HWND) {
    dispatch(hwnd, Op::Go(Target::Shown));
}

pub fn hide(hwnd: HWND) {
    dispatch(hwnd, Op::Go(Target::Hidden));
}

pub fn minimize(hwnd: HWND) {
    dispatch(hwnd, Op::Go(Target::Minimized));
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let motion = unsafe { &*(data as *const Motion) };
    if msg == WM_STYLECHANGING && wparam.0 as i32 == GWL_EXSTYLE.0 && motion.layered.get() {
        // Keep the fade across another component's extended style update.
        let style = unsafe { &mut *(lparam.0 as *mut STYLESTRUCT) };
        style.styleNew |= WS_EX_LAYERED.0;
    }
    if msg == WM_NCDESTROY {
        motion.stop_clock();
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(procedure), SUBCLASS);
            drop(Box::from_raw(data as *mut Motion));
            return DefSubclassProc(hwnd, msg, wparam, lparam);
        }
    }
    if msg == REQUEST {
        if let Some(op) = Op::decode(wparam, lparam) {
            perform(hwnd, op);
        }
        return LRESULT(0);
    }
    if msg == FRAME && wparam.0 == motion.id {
        if let Some(ticker) = motion.ticker.borrow().as_ref() {
            ticker.pending.store(false, Ordering::Release);
        }
        if let Some(mut fade) = motion.fade.get() {
            let (alpha, done) = fade.sample(Instant::now());
            motion.fade.set(Some(fade));
            if done || !motion.alpha(hwnd, alpha) {
                motion.finish(hwnd, fade.target);
            }
        }
        return LRESULT(0);
    }
    if msg == WM_SYSCOMMAND {
        match wparam.0 as u32 & 0xfff0 {
            SC_MINIMIZE => {
                motion.request(hwnd, Target::Minimized);
                return LRESULT(0);
            }
            SC_RESTORE
                if unsafe { IsIconic(hwnd).as_bool() }
                    || motion
                        .fade
                        .get()
                        .is_some_and(|fade| fade.target != Target::Shown) =>
            {
                motion.request(hwnd, Target::Shown);
                return LRESULT(0);
            }
            _ => {}
        }
    }
    let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    if msg == WM_STYLECHANGED && wparam.0 as i32 == GWL_EXSTYLE.0 && motion.layered.get() {
        // Rebuilding the style can reset the layered attributes even when
        // WS_EX_LAYERED is retained. Restore opacity before DWM can present it.
        motion.alpha(hwnd, motion.opacity.get());
    }
    if msg == WM_WINDOWPOSCHANGED && !changing_visibility() {
        let now = shown(hwnd);
        if motion.shown.replace(now) != now {
            if now && motion.enabled.get() && motion.fade.get().is_none() {
                // Also covers restores by the taskbar or another program that
                // calls ShowWindow directly instead of sending SC_RESTORE.
                if motion.alpha(hwnd, 0) {
                    motion.fade.set(Some(Fade::new(0, Target::Shown)));
                    if !motion.start_clock(hwnd) {
                        motion.finish(hwnd, Target::Shown);
                    }
                } else {
                    motion.opaque(hwnd);
                }
            } else if !now && motion.fade.get().is_some() {
                // A direct external hide/minimize supersedes the pending fade.
                motion.fade.set(None);
                LEAVING.store(false, Ordering::Relaxed);
                motion.stop_clock();
                motion.opaque(hwnd);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_fade_starts_at_the_current_opacity() {
        let start = Instant::now();
        let mut hide = Fade::new(255, Target::Hidden);
        assert_eq!(hide.sample(start), (255, false));
        let now = start + Duration::from_millis(70);
        let opacity = hide.sample(now).0;
        let mut show = Fade::new(opacity, Target::Shown);
        assert_eq!(show.sample(now), (opacity, false));
        assert!(show.sample(now + Duration::from_millis(10)).0 > opacity);
        assert_eq!(show.sample(now + Duration::from_secs(1)), (255, true));
    }

    #[test]
    fn fade_clamps_at_its_destination() {
        let start = Instant::now();
        for target in [Target::Hidden, Target::Minimized] {
            let mut fade = Fade::new(255, target);
            assert_eq!(fade.sample(start), (255, false));
            assert_eq!(fade.sample(start + Duration::from_millis(140)), (0, true));
        }
        assert_eq!(Fade::new(255, Target::Shown).sample(start), (255, true));
    }

    #[test]
    fn a_blocked_window_thread_does_not_consume_the_fade_before_its_first_frame() {
        let created = Instant::now();
        let mut fade = Fade::new(0, Target::Shown);
        let first_frame = created + Duration::from_secs(1);
        assert_eq!(fade.sample(first_frame), (0, false));
        let (alpha, done) = fade.sample(first_frame + Duration::from_millis(90));
        assert!(alpha > 0 && alpha < 255 && !done);
        assert_eq!(
            fade.sample(first_frame + Duration::from_millis(180)),
            (255, true)
        );
    }
}
