//! Records every presented frame of the owned window for a short span, so an
//! animation can be judged frame by frame. Each frame's crop is read back on
//! the capture thread and kept in memory; PNGs are written once recording ends.
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
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
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
            RO_INIT_MULTITHREADED, RoInitialize,
        },
    },
    core::Interface,
};

/// At most this many frames are kept, whatever the requested span.
const MAX_FRAMES: usize = 1200;

#[derive(Clone, Copy)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

struct Frame {
    time: i64,
    width: u32,
    height: u32,
    rgb: Vec<u8>,
}

pub struct Recorder {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<serde_json::Value>>,
}

impl Recorder {
    pub fn start(hwnd: HWND, dir: PathBuf, span: Duration, crop: Option<Crop>) -> Self {
        let handle = hwnd.0 as usize;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let thread = std::thread::spawn(move || {
            let frames = Arc::new(Mutex::new(Vec::new()));
            let result = record(HWND(handle as *mut _), span, crop, &frames, &worker_stop);
            let frames = std::mem::take(&mut *frames.lock().unwrap());
            let mut summary = serde_json::json!({
                "event": "recorded",
                "dir": dir.to_string_lossy(),
                "frames": frames.len(),
                "times100ns": frames.iter().map(|f| f.time).collect::<Vec<_>>(),
            });
            if let Err(error) = result {
                summary["error"] = format!("WGC HRESULT {:#010x}", error.code().0).into();
            }
            if let Err(error) = write(&dir, &frames) {
                summary["error"] = error.to_string().into();
            }
            summary
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
    /// Whether the recording span has elapsed (the thread is writing or done).
    pub fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }
    pub fn finish(mut self) -> serde_json::Value {
        self.stop.store(true, Ordering::Relaxed);
        self.thread
            .take()
            .and_then(|t| t.join().ok())
            .unwrap_or(serde_json::json!({"event":"recorded","error":"recorder panicked"}))
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn write(dir: &std::path::Path, frames: &[Frame]) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut index = Vec::with_capacity(frames.len());
    for (i, frame) in frames.iter().enumerate() {
        let name = format!("{i:04}.png");
        let mut encoder = png::Encoder::new(
            std::io::BufWriter::new(std::fs::File::create(dir.join(&name))?),
            frame.width,
            frame.height,
        );
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&frame.rgb)?;
        index.push(serde_json::json!({"file":name,"time100ns":frame.time}));
    }
    std::fs::write(
        dir.join("index.json"),
        serde_json::to_string(&serde_json::json!({ "frames": index }))?,
    )?;
    Ok(())
}

fn record(
    hwnd: HWND,
    span: Duration,
    crop: Option<Crop>,
    frames: &Arc<Mutex<Vec<Frame>>>,
    stop: &AtomicBool,
) -> windows::core::Result<()> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
    let (mut device, mut context) = (None, None);
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
            Some(&mut context),
        )?
    };
    let device = device.unwrap();
    let context = context.unwrap();
    let dxgi: IDXGIDevice = device.cast()?;
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
    let sink = frames.clone();
    let closing = Arc::new(AtomicBool::new(false));
    let closed = closing.clone();
    // One staging texture, recreated when the crop's size changes.
    let staging: Arc<Mutex<Option<(ID3D11Texture2D, u32, u32)>>> = Arc::new(Mutex::new(None));
    let guard = staging.clone();
    let token = pool.FrameArrived(&TypedEventHandler::<
        Direct3D11CaptureFramePool,
        windows::core::IInspectable,
    >::new(move |sender, _| {
        // Teardown takes this lock before closing the pool, so no callback
        // touches capture objects that are being closed.
        let mut slot = staging.lock().unwrap();
        if closed.load(Ordering::Relaxed) {
            return Ok(());
        }
        let Some(pool) = sender.as_ref() else {
            return Ok(());
        };
        while let Ok(frame) = pool.TryGetNextFrame() {
            let time = frame.SystemRelativeTime()?.Duration;
            let content = frame.ContentSize()?;
            let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
            let (cw, ch) = (content.Width as u32, content.Height as u32);
            let area = crop.unwrap_or(Crop {
                x: 0,
                y: 0,
                width: cw,
                height: ch,
            });
            let x = area.x.min(cw);
            let y = area.y.min(ch);
            let width = area.width.min(cw - x);
            let height = area.height.min(ch - y);
            if width == 0 || height == 0 {
                frame.Close()?;
                continue;
            }
            if slot.as_ref().is_none_or(|s| s.1 != width || s.2 != height) {
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                unsafe { texture.GetDesc(&mut desc) };
                desc.Width = width;
                desc.Height = height;
                desc.Usage = D3D11_USAGE_STAGING;
                desc.BindFlags = 0;
                desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                desc.MiscFlags = 0;
                let mut created = None;
                unsafe { device.CreateTexture2D(&desc, None, Some(&mut created))? };
                *slot = Some((created.unwrap(), width, height));
            }
            let (target, _, _) = slot.as_ref().unwrap();
            let region = D3D11_BOX {
                left: x,
                top: y,
                front: 0,
                right: x + width,
                bottom: y + height,
                back: 1,
            };
            unsafe {
                context.CopySubresourceRegion(target, 0, 0, 0, 0, &texture, 0, Some(&region));
            }
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            unsafe { context.Map(target, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };
            let stride = mapped.RowPitch as usize;
            let bytes = unsafe {
                std::slice::from_raw_parts(mapped.pData as *const u8, stride * height as usize)
            };
            let mut rgb = Vec::with_capacity((width * height * 3) as usize);
            for row in 0..height as usize {
                for col in 0..width as usize {
                    let i = row * stride + col * 4;
                    rgb.extend_from_slice(&[bytes[i + 2], bytes[i + 1], bytes[i]]);
                }
            }
            unsafe { context.Unmap(target, 0) };
            frame.Close()?;
            let mut list = sink.lock().unwrap();
            if list.len() < MAX_FRAMES {
                list.push(Frame {
                    time,
                    width,
                    height,
                    rgb,
                });
            }
        }
        Ok(())
    }))?;
    session.StartCapture()?;
    let started = Instant::now();
    while !stop.load(Ordering::Relaxed) && started.elapsed() < span {
        std::thread::sleep(Duration::from_millis(5));
    }
    {
        let _held = guard.lock().unwrap();
        closing.store(true, Ordering::Relaxed);
    }
    pool.RemoveFrameArrived(token)?;
    session.Close()?;
    pool.Close()?;
    Ok(())
}
