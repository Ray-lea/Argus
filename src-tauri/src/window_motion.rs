//! In-place window fades. Native visibility still drives WebView sleep and
//! sampling; the temporary layered style exists only while a fade is running.

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tauri::{Runtime, WebviewWindow};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute};
use windows::Win32::UI::Shell::{
    DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, IsIconic, IsWindowVisible, LWA_ALPHA, PostMessageW,
    SC_MINIMIZE, SC_RESTORE, STYLESTRUCT, SW_HIDE, SW_MINIMIZE, SW_RESTORE, SW_SHOW,
    SetLayeredWindowAttributes, SetWindowLongPtrW, ShowWindow, WM_APP, WM_NCDESTROY,
    WM_STYLECHANGED, WM_STYLECHANGING, WM_SYSCOMMAND, WM_WINDOWPOSCHANGED, WS_EX_LAYERED,
};

const SUBCLASS: usize = 1;
const FRAME: u32 = WM_APP + 0x46;
static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
static LEAVING: AtomicBool = AtomicBool::new(false);
static CHANGING: AtomicUsize = AtomicUsize::new(0);

pub fn changing_visibility() -> bool {
    CHANGING.load(Ordering::Relaxed) != 0
}

struct VisibilityChange;

impl Drop for VisibilityChange {
    fn drop(&mut self) {
        CHANGING.fetch_sub(1, Ordering::Relaxed);
    }
}

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
    apply: Box<dyn Fn(Target)>,
}

impl Motion {
    fn change(&self, hwnd: HWND, target: Target) {
        CHANGING.fetch_add(1, Ordering::Relaxed);
        let guard = VisibilityChange;
        (self.apply)(target);
        drop(guard);
        if !changing_visibility() {
            self.shown
                .set(unsafe { IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() });
            // tao can temporarily hide a maximized window while rebuilding
            // its styles. Report only the completed visibility change.
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
        let shown = unsafe { IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() };
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

fn apply(hwnd: HWND, target: Target) {
    unsafe {
        let command = match target {
            Target::Shown if IsIconic(hwnd).as_bool() => SW_RESTORE,
            Target::Shown => SW_SHOW,
            Target::Hidden => SW_HIDE,
            Target::Minimized => SW_MINIMIZE,
        };
        let _ = ShowWindow(hwnd, command);
    }
}

/// Installed on the window's thread before it can be minimized or restored.
pub fn install<R: Runtime>(window: &WebviewWindow<R>) {
    let Ok(hwnd) = window.hwnd() else { return };
    let hwnd = HWND(hwnd.0);
    let handle = window.clone();
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
            // The page sends the resolved interface-motion preference on startup.
            enabled: Cell::new(false),
            fade: Cell::new(None),
            layered: Cell::new(false),
            opacity: Cell::new(255),
            shown: Cell::new(IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool()),
            ticker: RefCell::new(None),
            apply: Box::new(move |target| {
                // Keep tao's cached visibility in sync too: set_focus consults
                // it even though is_visible queries the real HWND.
                match target {
                    Target::Shown => {
                        let _ = handle.show();
                        if IsIconic(hwnd).as_bool() {
                            let _ = handle.unminimize();
                        }
                    }
                    Target::Hidden => {
                        let _ = handle.hide();
                    }
                    Target::Minimized => {
                        let _ = handle.minimize();
                    }
                }
                // An external ShowWindow may have changed the HWND without
                // updating tao's cache, making the corresponding call a no-op.
                let matches = match target {
                    Target::Shown => IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool(),
                    Target::Hidden => !IsWindowVisible(hwnd).as_bool(),
                    Target::Minimized => {
                        IsWindowVisible(hwnd).as_bool() && IsIconic(hwnd).as_bool()
                    }
                };
                if !matches {
                    apply(hwnd, target);
                }
            }),
        }));
        if !SetWindowSubclass(hwnd, Some(procedure), SUBCLASS, motion as usize).as_bool() {
            drop(Box::from_raw(motion));
        }
    }
}

fn with_motion<R: Runtime>(
    window: &WebviewWindow<R>,
    action: impl FnOnce(HWND, Option<&Motion>) + Send + 'static,
) {
    let Ok(hwnd) = window.hwnd() else { return };
    let handle = hwnd.0 as usize;
    let _ = window.run_on_main_thread(move || {
        let hwnd = HWND(handle as _);
        let mut data = 0;
        let found = unsafe { GetWindowSubclass(hwnd, Some(procedure), SUBCLASS, Some(&mut data)) }
            .as_bool();
        action(hwnd, found.then(|| unsafe { &*(data as *const Motion) }));
    });
}

pub fn set_enabled<R: Runtime>(window: &WebviewWindow<R>, enabled: bool) {
    with_motion(window, move |hwnd, motion| {
        if let Some(motion) = motion {
            motion.enabled.set(enabled);
            if !enabled && let Some(fade) = motion.fade.get() {
                motion.finish(hwnd, fade.target);
            }
        }
    });
}

fn request<R: Runtime>(window: &WebviewWindow<R>, target: Target) {
    with_motion(window, move |hwnd, motion| {
        if let Some(motion) = motion {
            motion.request(hwnd, target);
        } else {
            apply(hwnd, target);
        }
    });
}

pub fn show<R: Runtime>(window: &WebviewWindow<R>) {
    request(window, Target::Shown);
}

pub fn hide<R: Runtime>(window: &WebviewWindow<R>) {
    request(window, Target::Hidden);
}

pub fn minimize<R: Runtime>(window: &WebviewWindow<R>) {
    request(window, Target::Minimized);
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
        // tao rebuilds extended styles when its visibility flag changes.
        // Preserve the fade across that update, including the very first show.
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
        let shown = unsafe { IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() };
        if motion.shown.replace(shown) != shown {
            if shown && motion.enabled.get() && motion.fade.get().is_none() {
                // Also covers restores by the taskbar or a second program that
                // calls ShowWindow directly instead of sending SC_RESTORE.
                if motion.alpha(hwnd, 0) {
                    motion.fade.set(Some(Fade::new(0, Target::Shown)));
                    if !motion.start_clock(hwnd) {
                        motion.finish(hwnd, Target::Shown);
                    }
                } else {
                    motion.opaque(hwnd);
                }
            } else if !shown && motion.fade.get().is_some() {
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
