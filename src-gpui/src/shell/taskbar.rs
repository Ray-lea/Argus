//! The taskbar monitor: a layered child window of a display's taskbar
//! (`Shell_TrayWnd` on the primary display, `Shell_SecondaryTrayWnd` on
//! the others) that shows short readings beside the notification area (or
//! where a taskbar without one keeps its clock), or at the start of the
//! taskbar (Windows 11 with its buttons centred). While the display chosen
//! is away or shows no taskbar, it sits on the primary display's and looks
//! for the one chosen every two seconds.
//!
//! A child window of another thread's window shares that thread's input
//! queue, and the taskbar's thread belongs to the shell. The monitor's window
//! therefore lives on a thread of its own with a message loop of its own:
//! a busy interface never holds up the taskbar, and a shell that stops
//! answering holds up only this thread. The window is made with
//! `WS_EX_NOPARENTNOTIFY`, so that making and closing it sends nothing to the
//! shell, and what a click asks for is posted to the main window, whose
//! thread acts on it: a click shows or hides Argus, as on the tray icon; a
//! right click opens the monitor's panel beside it.
//!
//! Windows 11 draws the taskbar's buttons in content of its own; the monitor
//! lies over its free space beside the tray, or at its start, past the
//! widgets button. Windows 10 gives the space before the tray to the task
//! buttons (`MSTaskSwWClass`): the monitor narrows them by its own width and
//! gives the width back when it goes. The shell may widen them again (when a
//! window opens, say); the next check narrows them once more.
//!
//! A test instance never touches the shell: its monitor attaches to a
//! stand-in taskbar of its own, shown at the bottom of the z-order without
//! activation (`--test`); for displays a test names (`displays::set_test`),
//! one stand-in each, stacked up from the bottom of the primary display.
use super::taskbar_draw::{Group, Layout, Painter, Reserve, lay_out};
use crate::settings::{TaskbarPlace, TaskbarStyle};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

/// What the monitor's window posts to the main window, the action in
/// `wParam` (`Action`).
pub(crate) const MESSAGE: u32 = WM_APP + 83;
/// The state changed (`Monitor::update`).
const CHANGED: u32 = WM_APP + 1;
/// The monitor goes.
const CLOSE: u32 = WM_APP + 2;
/// How often the taskbar's geometry and the shell's window are checked.
const CHECK: Duration = Duration::from_millis(500);
const TIMER: usize = 1;

/// What a pointer on the monitor asks of the main window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum Action {
    /// A click: show or hide the window.
    Toggle = 1,
    /// A right click: the monitor's panel.
    Context = 2,
    /// The pointer reached the monitor: its panel may follow.
    Warm = 3,
    /// The right button went down on the monitor.
    Press = 4,
}

impl Action {
    pub fn from(value: usize) -> Option<Self> {
        Some(match value {
            1 => Action::Toggle,
            2 => Action::Context,
            3 => Action::Warm,
            4 => Action::Press,
            _ => return None,
        })
    }
}

/// The monitor's window on screen, physical pixels (left, top, right,
/// bottom); none while it is not shown.
static SHOWN: Mutex<Option<[i32; 4]>> = Mutex::new(None);
/// The monitor shows readings, so sampling keeps the shown window's pace.
static LIVE: AtomicBool = AtomicBool::new(false);
/// While it shows: the display chosen for it (empty for the primary one)
/// and whether the primary display's taskbar stands in for that display's.
static ON: Mutex<Option<(String, bool)>> = Mutex::new(None);

pub(crate) fn shown_rect() -> Option<[i32; 4]> {
    *SHOWN.lock().unwrap()
}

pub(crate) fn live() -> bool {
    LIVE.load(Ordering::Relaxed)
}

pub(crate) fn shown_on() -> Option<(String, bool)> {
    ON.lock().unwrap().clone()
}

#[derive(Clone)]
struct State {
    place: TaskbarPlace,
    style: TaskbarStyle,
    /// The display chosen, by its key (`displays::Display::key`); empty
    /// for the primary display.
    display: String,
    groups: Vec<Group>,
    closed: bool,
}

struct Shared {
    state: Mutex<State>,
    /// The worker's control window, `HWND` as a number; 0 until it exists.
    control: AtomicIsize,
    done: (Mutex<bool>, Condvar),
}

impl Shared {
    fn post(&self, message: u32) {
        let control = self.control.load(Ordering::Acquire);
        if control != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(control as _)), message, WPARAM(0), LPARAM(0));
            }
        }
    }
}

/// The running monitor; dropping it does not stop it, `close` does.
pub(crate) struct Monitor {
    shared: Arc<Shared>,
}

impl Monitor {
    /// Starts the monitor's thread; clicks on it are posted to `main`.
    pub fn spawn(
        main: HWND,
        place: TaskbarPlace,
        style: TaskbarStyle,
        display: String,
        test: bool,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                place,
                style,
                display,
                groups: Vec::new(),
                closed: false,
            }),
            control: AtomicIsize::new(0),
            done: (Mutex::new(false), Condvar::new()),
        });
        let worker = shared.clone();
        let main = main.0 as isize;
        std::thread::Builder::new()
            .name("argus-taskbar".into())
            .spawn(move || {
                run(worker.clone(), HWND(main as _), test);
                LIVE.store(false, Ordering::Relaxed);
                *SHOWN.lock().unwrap() = None;
                *ON.lock().unwrap() = None;
                *worker.done.0.lock().unwrap() = true;
                worker.done.1.notify_all();
            })?;
        Ok(Self { shared })
    }

    fn update(&self, change: impl FnOnce(&mut State)) {
        change(&mut self.shared.state.lock().unwrap());
        self.shared.post(CHANGED);
    }

    pub fn place(&self, place: TaskbarPlace, style: TaskbarStyle, display: String) {
        self.update(|state| {
            state.place = place;
            state.style = style;
            state.display = display;
        });
    }

    /// New readings, as the monitor's metrics show them.
    pub fn readings(&self, groups: Vec<Group>) {
        let mut state = self.shared.state.lock().unwrap();
        if state.groups == groups {
            return;
        }
        state.groups = groups;
        drop(state);
        self.shared.post(CHANGED);
    }

    /// Takes the monitor off the taskbar, waiting at most `wait` for it.
    pub fn close(&self, wait: Duration) {
        self.update(|state| state.closed = true);
        self.shared.post(CLOSE);
        let (lock, done) = &self.shared.done;
        let finished = lock.lock().unwrap();
        let _ = done.wait_timeout_while(finished, wait, |finished| !*finished);
    }
}

/// The taskbar the monitor sits on and what it needs to know of it.
struct Host {
    window: HWND,
    /// The stand-in of a test instance, owned by this thread.
    stand_in: bool,
    /// The display chosen when it was found, by its key.
    wanted: String,
    /// The primary display's, standing in for the one chosen.
    fallback: bool,
    /// The primary display's taskbar, with the notification area and the
    /// widgets button.
    primary: bool,
}

/// A taskbar found for the display chosen: its window, whether the primary
/// display's stands in for it, and whether it is the primary display's.
struct Found {
    window: HWND,
    fallback: bool,
    primary: bool,
}

/// The Windows 10 task buttons narrowed for the monitor: their window, its
/// right edge before and the one it was left with (screen pixels).
struct Squeeze {
    tasks: HWND,
    right: i32,
    left_at: i32,
}

struct Worker {
    shared: Arc<Shared>,
    main: HWND,
    test: bool,
    host: Option<Host>,
    /// The test stand-ins, by the key of the display each is for, the
    /// primary display's first; made again when the displays change.
    stand_ins: Vec<(String, HWND)>,
    window: Option<HWND>,
    painter: Option<Painter>,
    layout: Layout,
    /// The image last given to the window, and where (host client pixels).
    image: Vec<u8>,
    placed: Option<(i32, i32, i32, i32)>,
    light: bool,
    hover: bool,
    tracking: bool,
    squeeze: Option<Squeeze>,
    /// How wide the capsules' values have been lately.
    reserve: Reserve,
    /// When the shell's taskbar was last looked for while missing.
    sought: Option<Instant>,
    failed: bool,
}

thread_local! {
    static WORKER: RefCell<Option<Worker>> = const { RefCell::new(None) };
}

fn with_worker<R>(f: impl FnOnce(&mut Worker) -> R) -> Option<R> {
    WORKER.with(|worker| worker.try_borrow_mut().ok()?.as_mut().map(f))
}

fn instance() -> HINSTANCE {
    unsafe { GetModuleHandleW(None) }
        .map(|module| module.into())
        .unwrap_or_default()
}

fn register(class: PCWSTR, procedure: WNDPROC, background: Option<HBRUSH>) {
    unsafe {
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: procedure,
            hInstance: instance(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: background.unwrap_or_default(),
            ..Default::default()
        });
    }
}

const CONTROL_CLASS: PCWSTR = w!("ArgusTaskbarMonitorControl");
const WINDOW_CLASS: PCWSTR = w!("ArgusTaskbarMonitor");
/// The test stand-in's classes: its tray goes by the shell's class name,
/// local to this process, so that finding it takes the same path.
const STAND_IN_CLASS: PCWSTR = w!("ArgusTestTaskbar");
const TRAY_CLASS: PCWSTR = w!("TrayNotifyWnd");

fn run(shared: Arc<Shared>, main: HWND, test: bool) {
    register(CONTROL_CLASS, Some(control_procedure), None);
    register(WINDOW_CLASS, Some(window_procedure), None);
    let control = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CONTROL_CLASS,
            w!("Argus"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance()),
            None,
        )
    };
    let Ok(control) = control else {
        argus_core::diagnostics::warn("taskbar monitor control window", "creation failed");
        return;
    };
    unsafe {
        // An elevated Argus still hears the shell restart.
        let _ = ChangeWindowMessageFilterEx(
            control,
            super::taskbar_created_message(),
            MSGFLT_ALLOW,
            None,
        );
        SetTimer(Some(control), TIMER, CHECK.as_millis() as u32, None);
    }
    shared.control.store(control.0 as isize, Ordering::Release);
    WORKER.with(|worker| {
        *worker.borrow_mut() = Some(Worker {
            shared: shared.clone(),
            main,
            test,
            host: None,
            stand_ins: Vec::new(),
            window: None,
            painter: None,
            layout: Layout::default(),
            image: Vec::new(),
            placed: None,
            light: super::taskbar_light(),
            hover: false,
            tracking: false,
            squeeze: None,
            reserve: Reserve::default(),
            sought: None,
            failed: false,
        })
    });
    // Closed before the control window existed to hear it.
    if !shared.state.lock().unwrap().closed {
        with_worker(|worker| worker.refresh());
        let mut message = MSG::default();
        while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    let worker = WORKER.with(|worker| worker.borrow_mut().take());
    if let Some(mut worker) = worker {
        worker.detach();
        worker.drop_stand_ins();
    }
    shared.control.store(0, Ordering::Release);
    unsafe {
        let _ = DestroyWindow(control);
    }
}

unsafe extern "system" fn control_procedure(
    hwnd: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    match message {
        CHANGED | WM_TIMER => {
            with_worker(|worker| worker.refresh());
            LRESULT(0)
        }
        CLOSE => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        WM_SETTINGCHANGE | WM_DISPLAYCHANGE => {
            if message == WM_DISPLAYCHANGE {
                super::displays::forget();
            }
            with_worker(|worker| {
                // A display came or went: the taskbar chosen may have too.
                worker.sought = None;
                worker.light = super::taskbar_light();
                worker.image.clear();
                worker.refresh();
            });
            LRESULT(0)
        }
        _ if message != 0 && message == super::taskbar_created_message() => {
            // The shell restarted: its taskbar is a new window.
            with_worker(|worker| {
                worker.detach();
                worker.sought = None;
                worker.refresh();
            });
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, w, l) },
    }
}

unsafe extern "system" fn window_procedure(
    hwnd: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    if super::ignores_pointer(message) {
        return super::ignored_pointer(message);
    }
    match message {
        WM_NCHITTEST => LRESULT(HTCLIENT as isize),
        // A click leaves the active window active, as on the tray icon.
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            with_worker(|worker| worker.pointer_moved(hwnd));
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with_worker(|worker| worker.pointer_left());
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            with_worker(|worker| worker.ask(Action::Toggle));
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            with_worker(|worker| worker.ask(Action::Context));
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            with_worker(|worker| worker.ask(Action::Press));
            LRESULT(0)
        }
        // Handled here: passed on, they would reach the taskbar and open
        // its own menu.
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_RBUTTONDBLCLK | WM_CONTEXTMENU => LRESULT(0),
        WM_DESTROY => {
            with_worker(|worker| {
                if worker.window == Some(hwnd) {
                    worker.window = None;
                    worker.placed = None;
                    worker.image.clear();
                    worker.shown(None);
                }
            });
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, w, l) },
    }
}

unsafe extern "system" fn stand_in_procedure(
    hwnd: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    match message {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => {
            let light = super::taskbar_light();
            let mut tray = [0u16; 32];
            let length = unsafe { GetClassNameW(hwnd, &mut tray) } as usize;
            let tray = String::from_utf16_lossy(&tray[..length]) == "TrayNotifyWnd";
            let color = match (light, tray) {
                (_, false) => super::taskbar_color(light),
                (true, true) => 0x00e3_e3e3,
                (false, true) => 0x0030_3030,
            };
            let mut rect = RECT::default();
            unsafe {
                let _ = GetClientRect(hwnd, &mut rect);
                let brush = CreateSolidBrush(COLORREF(color));
                FillRect(HDC(w.0 as _), &rect, brush);
                let _ = DeleteObject(brush.into());
            }
            LRESULT(1)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, w, l) },
    }
}

/// The physical pixels of a window, as left, top, right, bottom.
fn rect_of(window: HWND) -> Option<[i32; 4]> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(window, &mut rect) }.ok()?;
    Some([rect.left, rect.top, rect.right, rect.bottom])
}

fn alive(window: HWND) -> bool {
    unsafe { IsWindow(Some(window)) }.as_bool()
}

impl Worker {
    /// The test stand-ins: taskbar-sized strips at the bottom of the
    /// primary display, one for each display a test names (else one for the
    /// primary display), the primary display's lowest and with a tray at its
    /// right end; beneath every other window and never active.
    fn stand_ins(&mut self) -> &[(String, HWND)] {
        let displays = if super::displays::testing() {
            super::displays::displays()
        } else {
            super::displays::displays()
                .into_iter()
                .filter(|display| display.primary)
                .take(1)
                .collect()
        };
        let keys = displays
            .iter()
            .map(|display| display.key.clone())
            .collect::<Vec<_>>();
        let current = self.stand_ins.len() == keys.len().max(1)
            && self.stand_ins.iter().all(|(_, window)| alive(*window))
            && self
                .stand_ins
                .iter()
                .zip(&keys)
                .all(|((made, _), key)| made == key);
        if current {
            return &self.stand_ins;
        }
        self.drop_stand_ins();
        register(STAND_IN_CLASS, Some(stand_in_procedure), None);
        register(TRAY_CLASS, Some(stand_in_procedure), None);
        let monitor = unsafe { MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = unsafe { GetMonitorInfoW(monitor, &mut info) };
        let scale = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() }.max(96) as f32 / 96.;
        let px = |logical: f32| (logical * scale).round() as i32;
        let whole = info.rcMonitor;
        let (width, height) = ((whole.right - whole.left).min(px(1280.)), px(48.));
        let keys = if keys.is_empty() {
            vec![String::new()]
        } else {
            keys
        };
        for (index, key) in keys.into_iter().enumerate() {
            // "Argus taskbar stand-in" for the primary display, then
            // "… stand-in 2", "… stand-in 3", for captures to tell apart.
            let title = if index == 0 {
                "Argus taskbar stand-in".to_owned()
            } else {
                format!("Argus taskbar stand-in {}", index + 1)
            };
            let title = windows::core::HSTRING::from(title);
            unsafe {
                let Ok(window) = CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    STAND_IN_CLASS,
                    &title,
                    WS_POPUP | WS_CLIPCHILDREN,
                    whole.left,
                    whole.bottom - height * (index as i32 + 1),
                    width,
                    height,
                    None,
                    None,
                    Some(instance()),
                    None,
                ) else {
                    continue;
                };
                // Only the primary display's taskbar has the tray.
                if index == 0 {
                    let _ = CreateWindowExW(
                        WS_EX_NOPARENTNOTIFY,
                        TRAY_CLASS,
                        PCWSTR::null(),
                        WS_CHILD | WS_VISIBLE,
                        width - px(280.),
                        0,
                        px(280.),
                        height,
                        Some(window),
                        None,
                        Some(instance()),
                        None,
                    );
                }
                let _ = SetWindowPos(
                    window,
                    Some(HWND_BOTTOM),
                    0,
                    0,
                    0,
                    0,
                    SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                );
                self.stand_ins.push((key, window));
            }
        }
        &self.stand_ins
    }

    fn drop_stand_ins(&mut self) {
        for (_, window) in self.stand_ins.drain(..) {
            unsafe {
                let _ = DestroyWindow(window);
            }
        }
    }

    /// The stand-in for the display `wanted` names, or the primary
    /// display's.
    fn stand_in_for(&mut self, wanted: &str) -> Option<Found> {
        let stand_ins = self.stand_ins();
        let primary = stand_ins.first()?.1;
        Some(
            match stand_ins
                .iter()
                .position(|(key, _)| !wanted.is_empty() && key == wanted)
            {
                Some(index) => Found {
                    window: stand_ins[index].1,
                    fallback: false,
                    primary: index == 0,
                },
                None => Found {
                    window: primary,
                    fallback: !wanted.is_empty(),
                    primary: true,
                },
            },
        )
    }

    /// The taskbar to attach to for the display `wanted` names: looked for
    /// again while it is missing, or while the primary display's stands in
    /// for it (at most every two seconds). A taskbar other than the one the
    /// monitor sits on takes it over.
    fn find_host(&mut self, wanted: &str) -> Option<HWND> {
        // A display a test took away takes its stand-in along, as the shell
        // closes a display's taskbar.
        if self.test {
            self.stand_ins();
        }
        let current = self
            .host
            .as_ref()
            .filter(|host| alive(host.window) && host.wanted == wanted)
            .map(|host| (host.window, host.fallback));
        let lately = self
            .sought
            .is_some_and(|at| at.elapsed() < Duration::from_secs(2));
        match current {
            Some((window, false)) => return Some(window),
            Some((window, true)) if lately => return Some(window),
            None if lately && !self.test => return None,
            _ => {}
        }
        self.sought = Some(Instant::now());
        let found = if self.test {
            self.stand_in_for(wanted)
        } else {
            shell_taskbar(wanted)
        };
        let Some(found) = found else {
            if self.host.take().is_some() {
                self.detach();
            }
            return None;
        };
        if self.host.as_ref().map(|host| host.window) != Some(found.window) {
            // The monitor's window is a child of the taskbar it was made on.
            self.detach();
        }
        self.host = Some(Host {
            window: found.window,
            stand_in: self.test,
            wanted: wanted.to_owned(),
            fallback: found.fallback,
            primary: found.primary,
        });
        Some(found.window)
    }

    /// Makes the monitor's window on `host`.
    fn attach(&mut self, host: HWND) -> Option<HWND> {
        if let Some(window) = self.window.filter(|window| alive(*window)) {
            return Some(window);
        }
        let window = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_NOPARENTNOTIFY,
                WINDOW_CLASS,
                w!("Argus"),
                WS_CHILD | WS_CLIPSIBLINGS,
                0,
                0,
                1,
                1,
                Some(host),
                None,
                Some(instance()),
                None,
            )
        };
        match window {
            Ok(window) => {
                self.window = Some(window);
                self.placed = None;
                self.image.clear();
                self.failed = false;
                log::info!(target: "argus_taskbar", "taskbar monitor attached");
                Some(window)
            }
            Err(error) => {
                if !std::mem::replace(&mut self.failed, true) {
                    argus_core::diagnostics::warn(
                        "taskbar monitor window",
                        format!("code={:#x}", error.code().0),
                    );
                }
                None
            }
        }
    }

    /// Takes the monitor's window off the taskbar and gives the task buttons
    /// back their width.
    fn detach(&mut self) {
        self.release_squeeze();
        if let Some(window) = self.window.take() {
            unsafe {
                let _ = DestroyWindow(window);
            }
        }
        self.placed = None;
        self.image.clear();
        self.hover = false;
        self.tracking = false;
        self.shown(None);
    }

    fn shown(&self, rect: Option<[i32; 4]>) {
        *SHOWN.lock().unwrap() = rect;
        *ON.lock().unwrap() = rect.and_then(|_| {
            let host = self.host.as_ref()?;
            Some((host.wanted.clone(), host.fallback))
        });
        let before = LIVE.swap(rect.is_some(), Ordering::Relaxed);
        // On the taskbar while the window is away: a fresh sample at the
        // faster pace.
        if rect.is_some()
            && !before
            && let Some(app) = super::HOST.get()
        {
            app.state::<crate::AppState>()
                .sampler
                .send(crate::sampler::Control::Refresh);
        }
    }

    /// Lays the monitor out, places it and draws it as the state, the
    /// taskbar and the pointer now are.
    fn refresh(&mut self) {
        let state = self.shared.state.lock().unwrap().clone();
        if state.closed {
            return;
        }
        let Some(host) = self.find_host(&state.display) else {
            self.detach();
            return;
        };
        let Some(bar) = rect_of(host) else {
            return;
        };
        let (width, height) = (bar[2] - bar[0], bar[3] - bar[1]);
        // A vertical taskbar has no room for columns of readings.
        if state.groups.is_empty() || height > width || height <= 0 {
            self.detach();
            return;
        }
        let Some(window) = self.attach(host) else {
            return;
        };
        let scale = unsafe { GetDpiForWindow(host) }.max(96) as f32 / 96.;
        let font = super::taskbar_draw::font_size(super::taskbar_draw::size_scale(height, scale));
        if self
            .painter
            .as_ref()
            .is_none_or(|painter| painter.font != font)
        {
            self.painter = Painter::new(font);
            self.image.clear();
        }
        let Some(painter) = &self.painter else {
            return;
        };
        let layout = lay_out(
            &state.groups,
            height,
            scale,
            state.style,
            painter,
            Some((&mut self.reserve, Instant::now())),
        );
        if layout.width <= 0 {
            self.detach();
            return;
        }
        let stand_in = self.host.as_ref().is_some_and(|host| host.stand_in);
        let primary = self.host.as_ref().is_none_or(|host| host.primary);
        let geometry = Geometry::read(host, bar, scale, stand_in, primary);
        let x = match self.squeeze_for(&geometry, layout.width) {
            Some(x) => x,
            None => place(&geometry, layout.width, state.place),
        };
        let placed = (x, 0, layout.width, height);
        let on_top = unsafe { GetWindow(host, GW_CHILD) }.ok() == Some(window);
        if self.placed != Some(placed) || !on_top {
            unsafe {
                let _ = SetWindowPos(
                    window,
                    Some(HWND_TOP),
                    x,
                    0,
                    layout.width,
                    height,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
            self.placed = Some(placed);
        }
        if layout != self.layout || self.image.is_empty() {
            self.layout = layout;
            self.draw(window, scale);
        }
        self.shown(rect_of(window));
    }

    /// Draws the layout into the window, unless it shows that already.
    fn draw(&mut self, window: HWND, scale: f32) {
        let Some(painter) = &self.painter else {
            return;
        };
        let Some(image) = painter.paint(&self.layout, self.light, self.hover, scale) else {
            return;
        };
        if image == self.image {
            return;
        }
        let (width, height) = (self.layout.width, self.layout.height);
        if present(window, &image, width, height) {
            self.image = image;
        } else if !std::mem::replace(&mut self.failed, true) {
            argus_core::diagnostics::warn("taskbar monitor drawing", "layered update failed");
        }
    }

    fn redraw(&mut self) {
        if let Some(window) = self.window {
            let scale = self
                .host
                .as_ref()
                .map_or(96, |host| unsafe { GetDpiForWindow(host.window) })
                .max(96) as f32
                / 96.;
            self.draw(window, scale);
        }
    }

    /// The pointer moved over the monitor: it lights up until the pointer
    /// leaves, and the panel's window is made ahead of a right click.
    fn pointer_moved(&mut self, window: HWND) {
        if self.tracking {
            return;
        }
        unsafe {
            let mut track = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: window,
                dwHoverTime: 0,
            };
            self.tracking = TrackMouseEvent(&mut track).is_ok();
        }
        self.hover = true;
        self.redraw();
        self.ask(Action::Warm);
    }

    fn pointer_left(&mut self) {
        self.tracking = false;
        self.hover = false;
        self.redraw();
    }

    /// Passes what the pointer asks for to the main window's thread.
    fn ask(&self, action: Action) {
        unsafe {
            let _ = PostMessageW(Some(self.main), MESSAGE, WPARAM(action as usize), LPARAM(0));
        }
    }

    /// On Windows 10, narrows the task buttons to make room beside the
    /// tray, wherever the monitor was asked to go: the monitor's place in
    /// the taskbar, or none where the shell leaves free space (Windows 11)
    /// or the buttons cannot be found.
    fn squeeze_for(&mut self, geometry: &Geometry, width: i32) -> Option<i32> {
        if geometry.windows11 || geometry.stand_in {
            self.release_squeeze();
            return None;
        }
        let host = self.host.as_ref()?;
        // The primary display's buttons sit in a rebar, the other
        // displays' in a worker window.
        let (strip, buttons) = if host.primary {
            (w!("ReBarWindow32"), w!("MSTaskSwWClass"))
        } else {
            (w!("WorkerW"), w!("MSTaskListWClass"))
        };
        let rebar =
            unsafe { FindWindowExW(Some(host.window), None, strip, PCWSTR::null()) }.ok()?;
        let tasks = unsafe { FindWindowExW(Some(rebar), None, buttons, PCWSTR::null()) }.ok()?;
        let (strip, buttons) = (rect_of(rebar)?, rect_of(tasks)?);
        let gap = (4. * geometry.scale).round() as i32;
        // The shell keeps the strip clear of the tray; the monitor keeps
        // the same space from it as on Windows 11.
        let limit = geometry
            .tray
            .map_or(strip[2], |tray| strip[2].min(tray[0] - gap));
        let kept = self
            .squeeze
            .as_ref()
            .filter(|squeeze| squeeze.tasks == tasks)
            .map(|squeeze| (squeeze.right, squeeze.left_at));
        let right = unsqueezed(kept, buttons[2], limit);
        let edge = right - width - gap;
        if edge <= buttons[0] {
            self.release_squeeze();
            return None;
        }
        if buttons[2] != edge {
            unsafe {
                let _ = SetWindowPos(
                    tasks,
                    None,
                    buttons[0] - strip[0],
                    buttons[1] - strip[1],
                    edge - buttons[0],
                    buttons[3] - buttons[1],
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        // Where the buttons took it, should they keep a width of their own.
        let left_at = rect_of(tasks).map_or(edge, |buttons| buttons[2]);
        self.squeeze = Some(Squeeze {
            tasks,
            right,
            left_at,
        });
        Some(edge + gap - geometry.bar[0])
    }

    fn release_squeeze(&mut self) {
        let Some(squeeze) = self.squeeze.take() else {
            return;
        };
        let rebar = unsafe { GetParent(squeeze.tasks) }.ok();
        if let (Some(rebar), Some(buttons)) = (rebar, rect_of(squeeze.tasks))
            && let Some(strip) = rect_of(rebar)
            && squeeze.right > buttons[2]
        {
            unsafe {
                let _ = SetWindowPos(
                    squeeze.tasks,
                    None,
                    buttons[0] - strip[0],
                    buttons[1] - strip[1],
                    squeeze.right - buttons[0],
                    buttons[3] - buttons[1],
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
    }
}

/// Gives a layered window `image` (premultiplied BGRA) at its size.
fn present(window: HWND, image: &[u8], width: i32, height: i32) -> bool {
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let mut shown = false;
        if let Ok(bitmap) =
            CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        {
            if !bits.is_null() && image.len() == (width * height * 4) as usize {
                std::ptr::copy_nonoverlapping(image.as_ptr(), bits.cast::<u8>(), image.len());
                let old = SelectObject(memory, bitmap.into());
                let size = SIZE {
                    cx: width,
                    cy: height,
                };
                let origin = POINT::default();
                let blend = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                shown = UpdateLayeredWindow(
                    window,
                    Some(screen),
                    None,
                    Some(&size),
                    Some(memory),
                    Some(&origin),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                )
                .is_ok();
                SelectObject(memory, old);
            }
            let _ = DeleteObject(bitmap.into());
        }
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        shown
    }
}

/// The taskbar's layout, as far as the monitor's place depends on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Geometry {
    /// The taskbar, screen pixels.
    pub bar: [i32; 4],
    /// The notification area, screen pixels.
    pub tray: Option<[i32; 4]>,
    pub windows11: bool,
    /// The taskbar's buttons are centred (Windows 11).
    pub centred: bool,
    /// The widgets button shows (Windows 11, the primary display's
    /// taskbar, turned on and not disabled by group policy): at the start
    /// of a centred taskbar, beside the tray otherwise.
    pub widgets: bool,
    pub scale: f32,
    pub stand_in: bool,
}

impl Geometry {
    fn read(host: HWND, bar: [i32; 4], scale: f32, stand_in: bool, primary: bool) -> Self {
        let tray = unsafe { FindWindowExW(Some(host), None, w!("TrayNotifyWnd"), PCWSTR::null()) }
            .ok()
            .and_then(rect_of)
            .filter(|tray| tray[0] > bar[0] && tray[0] < bar[2]);
        // The stand-in is a Windows 11 taskbar with centred buttons and no
        // widgets, whatever the system.
        let shell = !stand_in && windows11();
        Self {
            bar,
            tray,
            windows11: shell || stand_in,
            centred: stand_in || (shell && explorer_flag(w!("TaskbarAl")).unwrap_or(true)),
            widgets: shell && primary && widgets_shown(),
            scale,
            stand_in,
        }
    }
}

/// The monitor's left edge in the taskbar's client pixels, `width` wide.
pub(crate) fn place(geometry: &Geometry, width: i32, place: TaskbarPlace) -> i32 {
    let px = |logical: f32| (logical * geometry.scale).round() as i32;
    let [left, _, right, _] = geometry.bar;
    // The widgets button with the weather beside it.
    let widgets = if geometry.widgets { px(160.) } else { 0 };
    if place == TaskbarPlace::Start && geometry.windows11 && geometry.centred {
        return px(4.) + widgets;
    }
    let end = match geometry.tray {
        Some(tray) => tray[0] - left - px(4.),
        // The clock and a few icons.
        None => right - left - px(160.),
    };
    let end = if geometry.windows11 && !geometry.centred {
        end - widgets
    } else {
        end
    };
    (end - width).max(0)
}

/// Where the Windows 10 task buttons end when not narrowed for the monitor,
/// screen pixels, as they end `now`: the edge remembered from before while
/// they still end where the monitor left them (`kept`: that edge, and where
/// they were left), else where they end now, the shell having laid them
/// out again as the tray grew or shrank. Never past `limit`.
fn unsqueezed(kept: Option<(i32, i32)>, now: i32, limit: i32) -> i32 {
    match kept {
        Some((right, left_at)) if now == left_at => right,
        _ => now,
    }
    .min(limit)
}

/// The shell's taskbar on the display `wanted` names (the primary
/// display's for none), or the primary display's while that display is away
/// or shows no taskbar of its own.
fn shell_taskbar(wanted: &str) -> Option<Found> {
    let primary = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.ok();
    let on_primary = |fallback| {
        primary.map(|window| Found {
            window,
            fallback,
            primary: true,
        })
    };
    if wanted.is_empty() {
        return on_primary(false);
    }
    let Some(display) = super::displays::displays()
        .into_iter()
        .find(|display| display.key == wanted)
    else {
        return on_primary(true);
    };
    if display.primary {
        return on_primary(false);
    }
    let mut after = None;
    while let Ok(window) =
        unsafe { FindWindowExW(None, after, w!("Shell_SecondaryTrayWnd"), PCWSTR::null()) }
    {
        if unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONULL) }.0 as isize == display.monitor
        {
            return Some(Found {
                window,
                fallback: false,
                primary: false,
            });
        }
        after = Some(window);
    }
    on_primary(true)
}

fn windows11() -> bool {
    super::os_build().is_some_and(|build| build >= 22000)
}

/// A DWORD value; none when unset.
fn registry_dword(
    hive: windows::Win32::System::Registry::HKEY,
    key: PCWSTR,
    name: PCWSTR,
) -> Option<u32> {
    use windows::Win32::System::Registry::*;
    let mut value = 0u32;
    let mut bytes = 4;
    unsafe {
        RegGetValueW(
            hive,
            key,
            name,
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
    }
    Some(value)
}

/// A DWORD of the shell's taskbar settings, as a flag; none when unset.
fn explorer_flag(name: PCWSTR) -> Option<bool> {
    registry_dword(
        windows::Win32::System::Registry::HKEY_CURRENT_USER,
        w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"),
        name,
    )
    .map(|value| value != 0)
}

/// Whether the taskbar's setting (`TaskbarDa`, none when never changed)
/// and the "Allow widgets" group policy (`AllowNewsAndInterests`, none when
/// not configured) leave the widgets button showing. The policy disabled
/// hides the button but leaves the setting as it was, and greys it out in
/// Settings; enabled, it lets the setting decide.
fn widgets_allowed(setting: Option<bool>, policy: Option<u32>) -> bool {
    setting != Some(false) && policy != Some(0)
}

/// Whether the taskbar shows the widgets button: allowed by the setting and
/// the group policy, with the widgets installed. Whether they are installed
/// is looked up once.
fn widgets_shown() -> bool {
    static INSTALLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let policy = registry_dword(
        windows::Win32::System::Registry::HKEY_LOCAL_MACHINE,
        w!(r"SOFTWARE\Policies\Microsoft\Dsh"),
        w!("AllowNewsAndInterests"),
    );
    widgets_allowed(explorer_flag(w!("TaskbarDa")), policy)
        && *INSTALLED.get_or_init(|| {
            std::env::var_os("LOCALAPPDATA").is_some_and(|local| {
                std::path::Path::new(&local)
                    .join("Packages")
                    .read_dir()
                    .is_ok_and(|mut packages| {
                        packages.any(|entry| {
                            entry.is_ok_and(|entry| {
                                entry
                                    .file_name()
                                    .to_string_lossy()
                                    .starts_with("MicrosoftWindows.Client.WebExperience")
                            })
                        })
                    })
            })
        })
}

#[cfg(test)]
mod tests {
    use super::{Geometry, place, unsqueezed, widgets_allowed};
    use crate::settings::TaskbarPlace;

    #[test]
    fn a_policy_that_disables_widgets_hides_the_button_the_setting_still_shows() {
        // Never changed, or turned on, with no policy: shown.
        assert!(widgets_allowed(None, None));
        assert!(widgets_allowed(Some(true), None));
        // Disabled by policy, the setting kept as it was: hidden.
        assert!(!widgets_allowed(None, Some(0)));
        assert!(!widgets_allowed(Some(true), Some(0)));
        // Enabled by policy, the setting decides.
        assert!(widgets_allowed(None, Some(1)));
        assert!(!widgets_allowed(Some(false), Some(1)));
        assert!(!widgets_allowed(Some(false), None));
    }

    #[test]
    fn narrowed_task_buttons_follow_the_tray_as_it_grows() {
        // First seen: the buttons end where the shell laid them out.
        assert_eq!(unsqueezed(None, 1800, 1830), 1800);
        // Narrowed to 1500 for the monitor, they still end there: the edge
        // from before holds.
        assert_eq!(unsqueezed(Some((1800, 1500)), 1500, 1830), 1800);
        // A tray icon came and the shell laid them out again, 24 pixels
        // short of before: the monitor must not reach into the tray.
        assert_eq!(unsqueezed(Some((1800, 1500)), 1776, 1806), 1776);
        // One went, and they reach further.
        assert_eq!(unsqueezed(Some((1800, 1500)), 1824, 1854), 1824);
        // Buttons that kept more width than asked do not walk the edge
        // back on every look.
        assert_eq!(unsqueezed(Some((1800, 1530)), 1530, 1830), 1800);
        // Never past the strip or into the tray.
        assert_eq!(unsqueezed(None, 1800, 1790), 1790);
        assert_eq!(unsqueezed(Some((1800, 1500)), 1500, 1790), 1790);
    }

    fn geometry() -> Geometry {
        Geometry {
            bar: [0, 1392, 2560, 1440],
            tray: Some([2047, 1392, 2560, 1440]),
            windows11: true,
            centred: true,
            widgets: false,
            scale: 1.,
            stand_in: false,
        }
    }

    #[test]
    fn the_monitor_sits_beside_the_tray_or_at_the_start() {
        let g = geometry();
        assert_eq!(place(&g, 200, TaskbarPlace::Tray), 2047 - 4 - 200);
        assert_eq!(place(&g, 200, TaskbarPlace::Start), 4);
        // The widgets button takes the start of a centred taskbar…
        let widgets = Geometry { widgets: true, ..g };
        assert_eq!(place(&widgets, 200, TaskbarPlace::Start), 164);
        assert_eq!(place(&widgets, 200, TaskbarPlace::Tray), 1843);
        // …and the end of one whose buttons start at the left, where the
        // start belongs to them.
        let left = Geometry {
            centred: false,
            ..widgets
        };
        assert_eq!(place(&left, 200, TaskbarPlace::Start), 2047 - 4 - 160 - 200);
        // Windows 10 always puts it beside the tray.
        let ten = Geometry {
            windows11: false,
            centred: false,
            widgets: false,
            ..g
        };
        assert_eq!(place(&ten, 200, TaskbarPlace::Start), 1843);
    }

    #[test]
    fn without_a_tray_it_leaves_room_for_the_clock_and_scales() {
        let g = Geometry {
            bar: [1920, 1032, 3840, 1080],
            tray: None,
            scale: 1.5,
            ..geometry()
        };
        assert_eq!(place(&g, 300, TaskbarPlace::Tray), 1920 - 240 - 300);
        // A tray outside the taskbar does not count.
        assert_eq!(
            place(&Geometry { tray: None, ..g }, 5000, TaskbarPlace::Tray),
            0
        );
    }
}
