//! A bounded CUDA compute workload for comparing telemetry. No window or settings.
use std::{
    ffi::c_void,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::HMODULE,
        System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
    },
    core::{PCSTR, s, w},
};
unsafe fn export<T: Copy>(module: HMODULE, name: PCSTR) -> T {
    let p = unsafe { GetProcAddress(module, name) }.expect("CUDA export");
    unsafe { std::mem::transmute_copy(&p) }
}
fn ok(code: i32) {
    assert_eq!(code, 0, "CUDA operation failed");
}
fn main() {
    unsafe {
        let module = LoadLibraryExW(w!("nvcuda.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)
            .expect("CUDA driver");
        let init: unsafe extern "system" fn(u32) -> i32 = export(module, s!("cuInit"));
        let device: unsafe extern "system" fn(*mut i32, i32) -> i32 =
            export(module, s!("cuDeviceGet"));
        let create: unsafe extern "system" fn(*mut *mut c_void, u32, i32) -> i32 =
            export(module, s!("cuCtxCreate_v2"));
        let destroy: unsafe extern "system" fn(*mut c_void) -> i32 =
            export(module, s!("cuCtxDestroy_v2"));
        let alloc: unsafe extern "system" fn(*mut u64, usize) -> i32 =
            export(module, s!("cuMemAlloc_v2"));
        let free: unsafe extern "system" fn(u64) -> i32 = export(module, s!("cuMemFree_v2"));
        let load: unsafe extern "system" fn(
            *mut *mut c_void,
            *const c_void,
            u32,
            *mut u32,
            *mut *mut c_void,
        ) -> i32 = export(module, s!("cuModuleLoadDataEx"));
        let unload: unsafe extern "system" fn(*mut c_void) -> i32 =
            export(module, s!("cuModuleUnload"));
        let function: unsafe extern "system" fn(*mut *mut c_void, *mut c_void, *const u8) -> i32 =
            export(module, s!("cuModuleGetFunction"));
        let launch: unsafe extern "system" fn(
            *mut c_void,
            u32,
            u32,
            u32,
            u32,
            u32,
            u32,
            u32,
            *mut c_void,
            *mut *mut c_void,
            *mut *mut c_void,
        ) -> i32 = export(module, s!("cuLaunchKernel"));
        let sync: unsafe extern "system" fn() -> i32 = export(module, s!("cuCtxSynchronize"));
        ok(init(0));
        let mut dev = 0;
        ok(device(&mut dev, 0));
        let mut context = std::ptr::null_mut();
        ok(create(&mut context, 4, dev));
        let ptx = concat!(
            r#".version 7.0
.target sm_75
.address_size 64
.visible .entry busy(.param .u64 output) {
 .reg .u64 %out,%offset;
 .reg .u32 %thread,%block,%width,%i;
 .reg .pred %more;
 .reg .f32 %v;
 ld.param.u64 %out,[output];
 mov.u32 %thread,%tid.x;mov.u32 %block,%ctaid.x;mov.u32 %width,%ntid.x;
 mad.lo.u32 %thread,%block,%width,%thread;cvt.rn.f32.u32 %v,%thread;mov.u32 %i,0;
 loop: fma.rn.f32 %v,%v,0f3f800001,0f3f000000;add.u32 %i,%i,1;setp.lt.u32 %more,%i,2000;@%more bra loop;
 mul.wide.u32 %offset,%thread,4;add.u64 %out,%out,%offset;st.global.f32 [%out],%v;ret;
}
"#,
            "\0"
        );
        let mut program = std::ptr::null_mut();
        let mut log = [0u8; 4096];
        let mut options = [5u32, 6];
        let mut values = [log.as_mut_ptr().cast(), log.len() as *mut c_void];
        let result = load(
            &mut program,
            ptx.as_ptr().cast(),
            2,
            options.as_mut_ptr(),
            values.as_mut_ptr(),
        );
        if result != 0 {
            eprintln!(
                "{}",
                String::from_utf8_lossy(
                    &log[..log.iter().position(|b| *b == 0).unwrap_or(log.len())]
                )
            );
        }
        ok(result);
        let mut kernel = std::ptr::null_mut();
        ok(function(&mut kernel, program, c"busy".as_ptr().cast()));
        let mut memory = 0;
        ok(alloc(&mut memory, 256 * 128 * 4));
        println!("{{\"pid\":{},\"phase\":\"idle\"}}", std::process::id());
        std::thread::sleep(Duration::from_secs(3));
        let deadline = Instant::now() + Duration::from_secs(12);
        let mut launches = 0;
        let mut longest = Duration::ZERO;
        println!("{{\"phase\":\"compute\",\"duty\":0.2}}");
        while Instant::now() < deadline {
            let burst = Instant::now() + Duration::from_millis(40);
            while Instant::now() < burst {
                let start = Instant::now();
                let mut args = [(&raw mut memory).cast()];
                ok(launch(
                    kernel,
                    256,
                    1,
                    1,
                    128,
                    1,
                    1,
                    0,
                    std::ptr::null_mut(),
                    args.as_mut_ptr(),
                    std::ptr::null_mut(),
                ));
                ok(sync());
                longest = longest.max(start.elapsed());
                launches += 1;
            }
            std::thread::sleep(Duration::from_millis(160));
        }
        println!(
            "{{\"phase\":\"idle\",\"launches\":{launches},\"maxKernelMs\":{}}}",
            longest.as_secs_f64() * 1000.0
        );
        std::thread::sleep(Duration::from_secs(3));
        ok(free(memory));
        ok(unload(program));
        ok(destroy(context));
    }
}
