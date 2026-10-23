//! Identical, GPU-resident window capture observes both renderers without a pixel readback.
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use windows::{
    Foundation::{TimeSpan, TypedEventHandler},
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Foundation::HWND,
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0},
            Direct3D11::*,
            Dxgi::IDXGIDevice,
        },
        System::WinRT::{
            Direct3D11::CreateDirect3D11DeviceFromDXGIDevice,
            Graphics::Capture::IGraphicsCaptureItemInterop, RO_INIT_MULTITHREADED, RoInitialize,
        },
    },
    core::Interface,
};

#[derive(Default)]
struct State {
    count: u64,
    times: Vec<i64>,
    error: Option<String>,
    ready: bool,
}
pub struct Observer {
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Observer {
    pub fn start(hwnd: HWND) -> Self {
        let handle = hwnd.0 as usize;
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_state = state.clone();
        let worker_stop = stop.clone();
        let thread = std::thread::spawn(move || {
            if let Err(error) = observe(HWND(handle as *mut _), &worker_state, &worker_stop) {
                worker_state.lock().unwrap().error =
                    Some(format!("WGC HRESULT {:#010x}", error.code().0));
            }
        });
        Self {
            state,
            stop,
            thread: Some(thread),
        }
    }
    pub fn sample(&self) -> serde_json::Value {
        let mut state = self.state.lock().unwrap();
        serde_json::json!({"count":state.count,"times100ns":std::mem::take(&mut state.times),"error":state.error,"ready":state.ready})
    }
}
impl Drop for Observer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn observe(hwnd: HWND, state: &Arc<Mutex<State>>, stop: &AtomicBool) -> windows::core::Result<()> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
    let mut device = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            Default::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )?
    };
    let dxgi: IDXGIDevice = device.unwrap().cast()?;
    let runtime: IDirect3DDevice =
        unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
    let factory: IGraphicsCaptureItemInterop =
        windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    let item: GraphicsCaptureItem = unsafe { factory.CreateForWindow(hwnd)? };
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &runtime,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        4,
        item.Size()?,
    )?;
    let session = pool.CreateCaptureSession(&item)?;
    session.SetIsCursorCaptureEnabled(false)?;
    session.SetMinUpdateInterval(TimeSpan { Duration: 0 })?;
    let observed = state.clone();
    let token = pool.FrameArrived(&TypedEventHandler::<
        Direct3D11CaptureFramePool,
        windows::core::IInspectable,
    >::new(move |sender, _| {
        if let Some(pool) = sender.as_ref() {
            while let Ok(frame) = pool.TryGetNextFrame() {
                let timestamp = frame.SystemRelativeTime()?.Duration;
                frame.Close()?;
                let mut state = observed.lock().unwrap();
                state.count += 1;
                if state.times.len() < 20_000 {
                    state.times.push(timestamp);
                } else {
                    state.error = Some("frame observer buffer overflow".into());
                }
            }
        }
        Ok(())
    }))?;
    session.StartCapture()?;
    state.lock().unwrap().ready = true;
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(50));
    }
    session.Close()?;
    pool.RemoveFrameArrived(token)?;
    pool.Close()?;
    Ok(())
}
