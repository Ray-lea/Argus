//! Application services and local notifications, independent of a browser runtime.
use serde::Serialize;
use std::{
    any::{Any, TypeId},
    collections::{HashMap, VecDeque},
    marker::PhantomData,
    ops::Deref,
    path::PathBuf,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicI32, AtomicIsize, Ordering},
    },
};
pub use url::Url;
use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};

pub type Result<T> = anyhow::Result<T>;
pub use argus_app::channel::Channel;
pub struct State<'a, T: Send + Sync + 'static>(Arc<T>, PhantomData<&'a T>);
impl<T: Send + Sync + 'static> Deref for State<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
pub struct Config {
    pub identifier: String,
}
pub struct Package {
    pub name: String,
}
#[derive(Clone)]
pub struct Paths {
    id: String,
}
impl Paths {
    fn directory(&self, base: &str) -> Result<PathBuf> {
        Ok(PathBuf::from(
            std::env::var_os(base)
                .ok_or_else(|| anyhow::anyhow!("application data directory unavailable"))?,
        )
        .join(&self.id))
    }
    pub fn app_config_dir(&self) -> Result<PathBuf> {
        self.directory("APPDATA")
    }
    pub fn app_local_data_dir(&self) -> Result<PathBuf> {
        self.directory("LOCALAPPDATA")
    }
    pub fn app_log_dir(&self) -> Result<PathBuf> {
        Ok(self.app_local_data_dir()?.join("logs"))
    }
}
#[derive(Clone, Debug)]
pub struct Event {
    pub name: String,
    pub value: serde_json::Value,
}
type MainTask = Box<dyn FnOnce() + Send>;
struct Shared {
    config: Config,
    package: Package,
    states: RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
    hwnd: AtomicIsize,
    events: Mutex<VecDeque<Event>>,
    main: Mutex<VecDeque<MainTask>>,
    wake: smol::channel::Sender<()>,
    receiver: smol::channel::Receiver<()>,
    exit: AtomicI32,
}
#[derive(Clone)]
pub struct Host(Arc<Shared>);
impl Host {
    pub fn new(identifier: String) -> Self {
        let (wake, receiver) = smol::channel::bounded(1);
        let name = if identifier == argus_app::config::identifier() {
            argus_app::config::product_name().to_owned()
        } else {
            format!("{} {identifier}", argus_app::config::product_name())
        };
        Self(Arc::new(Shared {
            config: Config { identifier },
            package: Package { name },
            states: RwLock::default(),
            hwnd: AtomicIsize::new(0),
            events: Mutex::default(),
            main: Mutex::default(),
            wake,
            receiver,
            exit: AtomicI32::new(i32::MIN),
        }))
    }
    pub fn config(&self) -> &Config {
        &self.0.config
    }
    pub fn package_info(&self) -> &Package {
        &self.0.package
    }
    pub fn path(&self) -> Paths {
        Paths {
            id: self.0.config.identifier.clone(),
        }
    }
    pub fn manage<T: Send + Sync + 'static>(&self, value: T) {
        self.0
            .states
            .write()
            .unwrap()
            .insert(TypeId::of::<T>(), Arc::new(value));
    }
    pub fn try_state<T: Send + Sync + 'static>(&self) -> Option<State<'_, T>> {
        self.0
            .states
            .read()
            .unwrap()
            .get(&TypeId::of::<T>())
            .cloned()?
            .downcast()
            .ok()
            .map(|v| State(v, PhantomData))
    }
    pub fn state<T: Send + Sync + 'static>(&self) -> State<'_, T> {
        self.try_state().expect("registered application state")
    }
    pub fn bind_window(&self, hwnd: HWND) {
        self.0.hwnd.store(hwnd.0 as isize, Ordering::Release);
    }
    pub fn get_window(&self, label: &str) -> Option<NativeWindow> {
        (label == "main" && self.0.hwnd.load(Ordering::Acquire) != 0)
            .then(|| NativeWindow { host: self.clone() })
    }
    pub fn emit(&self, name: &str, value: impl Serialize) -> Result<()> {
        let value = serde_json::to_value(value)?;
        let mut queue = self.0.events.lock().unwrap();
        if matches!(
            name,
            "update-status" | "argus-theme" | "argus-language" | "argus-caption-maximize"
        ) {
            queue.retain(|event| event.name != name);
        }
        queue.push_back(Event {
            name: name.into(),
            value,
        });
        drop(queue);
        self.wake();
        Ok(())
    }
    pub fn wake(&self) {
        let _ = self.0.wake.try_send(());
    }
    pub fn notifications(&self) -> smol::channel::Receiver<()> {
        self.0.receiver.clone()
    }
    pub fn drain(&self) -> Vec<Event> {
        let tasks: Vec<_> = self.0.main.lock().unwrap().drain(..).collect();
        for task in tasks {
            task();
        }
        self.0.events.lock().unwrap().drain(..).collect()
    }
    pub fn run_on_main_thread(&self, task: impl FnOnce() + Send + 'static) -> Result<()> {
        self.0.main.lock().unwrap().push_back(Box::new(task));
        self.wake();
        Ok(())
    }
    pub fn exit(&self, code: i32) {
        self.0.exit.store(code, Ordering::Release);
        self.wake();
    }
    pub fn exit_code(&self) -> Option<i32> {
        let code = self.0.exit.load(Ordering::Acquire);
        (code != i32::MIN).then_some(code)
    }
    pub fn autolaunch(&self) -> AutoLaunch {
        AutoLaunch {
            product: self.package_info().name.clone(),
        }
    }
}
#[derive(Clone)]
pub struct NativeWindow {
    host: Host,
}
impl NativeWindow {
    /// GPUI defers hidden-window placement until activation. Size it without showing or activating it.
    pub fn prepare_hidden_size(&self, width: f32, height: f32) -> Result<()> {
        use windows::Win32::{Foundation::RECT, UI::HiDpi::GetDpiForWindow};
        unsafe {
            let hwnd = self.hwnd()?;
            let mut client = RECT::default();
            let mut frame = RECT::default();
            GetClientRect(hwnd, &mut client)?;
            GetWindowRect(hwnd, &mut frame)?;
            let scale = GetDpiForWindow(hwnd) as f32 / 96.;
            SetWindowPos(
                hwnd,
                None,
                0,
                0,
                (width * scale).round() as i32 + frame.right - frame.left - client.right,
                (height * scale).round() as i32 + frame.bottom - frame.top - client.bottom,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOMOVE,
            )?;
        }
        Ok(())
    }
    pub fn hwnd(&self) -> Result<HWND> {
        let value = self.host.0.hwnd.load(Ordering::Acquire);
        anyhow::ensure!(value != 0, "window unavailable");
        Ok(HWND(value as _))
    }
    pub fn app_handle(&self) -> &Host {
        &self.host
    }

    pub fn show(&self) -> Result<()> {
        unsafe {
            let _ = ShowWindow(self.hwnd()?, SW_SHOWNOACTIVATE);
        }
        Ok(())
    }
    pub fn hide(&self) -> Result<()> {
        unsafe {
            let _ = ShowWindow(self.hwnd()?, SW_HIDE);
        }
        Ok(())
    }
    pub fn minimize(&self) -> Result<()> {
        unsafe {
            let _ = ShowWindow(self.hwnd()?, SW_MINIMIZE);
        }
        Ok(())
    }
    pub fn unminimize(&self) -> Result<()> {
        unsafe {
            let _ = ShowWindow(self.hwnd()?, SW_RESTORE);
        }
        Ok(())
    }
    pub fn is_visible(&self) -> Result<bool> {
        Ok(unsafe { IsWindowVisible(self.hwnd()?).as_bool() })
    }
    pub fn is_minimized(&self) -> Result<bool> {
        Ok(unsafe { IsIconic(self.hwnd()?).as_bool() })
    }

    pub fn is_focused(&self) -> Result<bool> {
        Ok(unsafe { GetForegroundWindow() == self.hwnd()? })
    }
    pub fn set_title(&self, title: &str) -> Result<()> {
        let text: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
        unsafe {
            SetWindowTextW(self.hwnd()?, windows::core::PCWSTR(text.as_ptr()))?;
        }
        Ok(())
    }
    pub fn set_focus(&self) -> Result<()> {
        unsafe {
            let _ = SetForegroundWindow(self.hwnd()?);
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(self.hwnd()?));
        }
        Ok(())
    }
}
pub struct AutoLaunch {
    product: String,
}
impl AutoLaunch {
    pub fn is_enabled(&self) -> Result<bool> {
        Ok(crate::commands::autostart::enabled(&self.product))
    }
    pub fn enable(&self) -> Result<()> {
        crate::commands::autostart::enable_current(&self.product).map_err(anyhow::Error::msg)
    }
    pub fn disable(&self) -> Result<()> {
        crate::commands::autostart::disable(&self.product).map_err(anyhow::Error::msg)
    }
}
