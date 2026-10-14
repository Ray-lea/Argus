//! Nuvoton NCT6779/NCT679x tachometers. Only index/bank selectors and the
//! temporary I/O-space access latch are written; PWM and fan curves are untouched.
use std::time::{Duration, Instant};

use argus_core::hardware::{CpuFanReading, CpuSensors};
use windows::Win32::Foundation::{WAIT_ABANDONED, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows::core::w;

use super::Module;
use crate::util::Handle;

const BLOB: &[u8] = include_bytes!("../../../vendor/pawnio/LpcIO.bin");
const FAN_REGISTERS: [u16; 7] = [0x4b0, 0x4b2, 0x4b4, 0x4b6, 0x4b8, 0x4ba, 0x4cc];

struct IsaGuard(Handle);
impl IsaGuard {
    fn acquire() -> Option<Self> {
        let handle = Handle(
            unsafe { CreateMutexW(None, false, w!(r"Global\Access_ISABUS.HTP.Method")) }.ok()?,
        );
        matches!(
            unsafe { WaitForSingleObject(handle.0, 20) },
            WAIT_OBJECT_0 | WAIT_ABANDONED
        )
        .then(|| Self(handle))
    }
}
impl Drop for IsaGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.0.0);
        }
    }
}

fn write_port(module: &Module, port: u16, value: u8) -> bool {
    module.call(b"ioctl_pio_outb", &[port as u64, value as u64], &mut [])
}
fn read_port(module: &Module, port: u16) -> Option<u8> {
    module
        .read(b"ioctl_pio_inb", &[port as u64])
        .and_then(|v| u8::try_from(v).ok())
}
fn read_config(module: &Module, register: u8) -> Option<u8> {
    module
        .read(b"ioctl_superio_inb", &[register as u64])
        .and_then(|v| u8::try_from(v).ok())
}
fn write_config(module: &Module, register: u8, value: u8) -> bool {
    module.call(
        b"ioctl_superio_outb",
        &[register as u64, value as u64],
        &mut [],
    )
}

trait ConfigIo {
    fn read(&self, register: u8) -> Option<u8>;
    fn write(&self, register: u8, value: u8) -> bool;
    fn port(&self, port: u16, value: u8) -> bool;
}
impl ConfigIo for Module {
    fn read(&self, register: u8) -> Option<u8> {
        read_config(self, register)
    }
    fn write(&self, register: u8, value: u8) -> bool {
        write_config(self, register, value)
    }
    fn port(&self, port: u16, value: u8) -> bool {
        write_port(self, port, value)
    }
}

struct Config<'a> {
    io: &'a dyn ConfigIo,
    port: u16,
    ldn: Option<u8>,
    options: Option<u8>,
    finished: bool,
}
impl<'a> Config<'a> {
    fn enter(io: &'a dyn ConfigIo, port: u16) -> Option<Self> {
        let mut value = Self {
            io,
            port,
            ldn: None,
            options: None,
            finished: false,
        };
        if !io.port(port, 0x87) || !io.port(port, 0x87) {
            return None;
        }
        value.ldn = io.read(0x07);
        value.ldn?;
        Some(value)
    }
    fn unlock(&mut self) -> Option<()> {
        if !self.io.write(0x07, 0x0b) {
            return None;
        }
        let options = self.io.read(0x28)?;
        self.options = Some(options);
        if options & 0x10 != 0 && !self.io.write(0x28, options & !0x10) {
            return None;
        }
        Some(())
    }
    fn exit_mode(&self) -> bool {
        self.io.port(self.port, 0xaa)
    }
    fn finish(&mut self) -> bool {
        // Re-enter because hardware-monitor reads happen outside configuration mode.
        let mut ok = self.io.port(self.port, 0x87);
        ok &= self.io.port(self.port, 0x87);
        if let Some(options) = self.options {
            ok &= self.io.write(0x07, 0x0b);
            ok &= self.io.write(0x28, options);
        }
        if let Some(ldn) = self.ldn {
            ok &= self.io.write(0x07, ldn);
        }
        ok &= self.exit_mode();
        self.finished = ok;
        ok
    }
}
impl Drop for Config<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.finish();
        }
    }
}

struct Chip {
    module: Module,
    port: u16,
    base: u16,
    name: &'static str,
    fans: usize,
    seen: u8,
}

#[derive(Default)]
pub(super) struct Board {
    chip: Option<Chip>,
    attempted: bool,
    failed_at: Option<Instant>,
}
impl Board {
    pub fn sample(&mut self, frame: &mut CpuSensors) {
        let Some(_isa) = IsaGuard::acquire() else {
            return;
        };
        if self.chip.is_none() && !self.attempted {
            self.attempted = true;
            self.chip = detect();
        }
        if self
            .failed_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(30))
        {
            return;
        }
        let Some(chip) = &mut self.chip else { return };
        let Some(mut config) = Config::enter(&chip.module, chip.port) else {
            return;
        };
        if config.unlock().is_none() || !config.exit_mode() {
            return;
        }
        let readings = chip.read();
        if !config.finish() || readings.is_none() {
            self.failed_at = Some(Instant::now());
            return;
        }
        frame.board_sensor = Some(chip.name.into());
        for (index, rpm) in readings.unwrap().into_iter().enumerate() {
            let Some(rpm) = rpm else { continue };
            if rpm > 0.0 {
                chip.seen |= 1 << index;
            }
            if chip.seen & (1 << index) != 0 {
                frame.board_fans.push(CpuFanReading {
                    channel: index as u8,
                    rpm,
                });
            }
        }
        self.failed_at = None;
    }
}

fn detect() -> Option<Chip> {
    for (slot, port) in [(0, 0x2e), (1, 0x4e)] {
        let module = Module::open(BLOB).ok()?;
        if !module.call(b"ioctl_select_slot", &[slot], &mut []) {
            continue;
        }
        let Some(mut config) = Config::enter(&module, port) else {
            continue;
        };
        let result = (|| {
            let id = module.read(b"ioctl_superio_inw", &[0x20])? as u16;
            let (name, fans) = chip_profile(id)?;
            if !module.call(b"ioctl_find_bars", &[], &mut []) || !write_config(&module, 0x07, 0x0b)
            {
                return None;
            }
            let base = module.read(b"ioctl_superio_inw", &[0x60])? as u16;
            let confirm = module.read(b"ioctl_superio_inw", &[0x60])? as u16;
            if base != confirm
                || !(0x100..=0xfff0).contains(&base)
                || base & 7 != 0
                || (0xcf8..=0xcff).contains(&base)
            {
                return None;
            }
            Some((base, name, fans))
        })();
        let restored = config.finish();
        drop(config);
        if let Some((base, name, fans)) = result.filter(|_| restored) {
            return Some(Chip {
                module,
                port,
                base,
                name,
                fans,
                seen: 0,
            });
        }
    }
    None
}

fn chip_profile(id: u16) -> Option<(&'static str, usize)> {
    Some(match id {
        0xc560..=0xc56f => ("NCT6779D", 5),
        0xc803 => ("NCT6791D", 6),
        0xc911 => ("NCT6792D", 6),
        0xc913 => ("NCT6792DA", 6),
        0xd121 => ("NCT6793D", 6),
        0xd352 => ("NCT6795D", 6),
        0xd423 => ("NCT6796D", 6),
        0xd42a => ("NCT6796DR", 7),
        0xd451 => ("NCT6797D", 7),
        0xd42b => ("NCT6798D", 7),
        0xd802 => ("NCT6799D", 7),
        _ => return None,
    })
}

impl Chip {
    fn byte(&self, register: u16) -> Option<u8> {
        if !write_port(&self.module, self.base + 5, 0x4e)
            || !write_port(&self.module, self.base + 6, (register >> 8) as u8)
            || !write_port(&self.module, self.base + 5, register as u8)
        {
            return None;
        }
        read_port(&self.module, self.base + 6)
    }
    fn read(&self) -> Option<Vec<Option<f32>>> {
        // Preserve the bank selector for other cooperating monitor programs.
        if !write_port(&self.module, self.base + 5, 0x4e) {
            return None;
        }
        let bank = read_port(&self.module, self.base + 6)?;
        let result = (|| {
            if u16::from_be_bytes([self.byte(0x804f)?, self.byte(0x004f)?]) != 0x5ca3 {
                return None;
            }
            let mut fans = Vec::with_capacity(self.fans);
            for &register in &FAN_REGISTERS[..self.fans] {
                let high = self.byte(register)?;
                let low = self.byte(register + 1)?;
                fans.push(fan_rpm(high, low));
            }
            Some(fans)
        })();
        if !write_port(&self.module, self.base + 5, 0x4e)
            || !write_port(&self.module, self.base + 6, bank)
        {
            return None;
        }
        result
    }
}

fn fan_rpm(high: u8, low: u8) -> Option<f32> {
    let count = (u16::from(high) << 5) | u16::from(low & 31);
    if count == 0 || count == 0x1fff {
        return Some(0.0);
    }
    super::valid(1_350_000.0 / f64::from(count), 0.0, 20000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeConfig {
        ldn: std::cell::Cell<u8>,
        options: std::cell::Cell<u8>,
        fail_options_read: bool,
        last_port: std::cell::Cell<u8>,
    }
    impl ConfigIo for FakeConfig {
        fn read(&self, register: u8) -> Option<u8> {
            match register {
                7 => Some(self.ldn.get()),
                0x28 if !self.fail_options_read => Some(self.options.get()),
                _ => None,
            }
        }
        fn write(&self, register: u8, value: u8) -> bool {
            match register {
                7 => self.ldn.set(value),
                0x28 => self.options.set(value),
                _ => panic!("unexpected config write"),
            }
            true
        }
        fn port(&self, port: u16, value: u8) -> bool {
            assert_eq!(port, 0x2e);
            assert!(matches!(value, 0x87 | 0xaa));
            self.last_port.set(value);
            true
        }
    }
    #[test]
    fn restores_monitor_access_latch_and_logical_device_on_success_and_failure() {
        for fail in [false, true] {
            let io = FakeConfig {
                ldn: 4.into(),
                options: 0xb5.into(),
                fail_options_read: fail,
                last_port: 0.into(),
            };
            {
                let mut config = Config::enter(&io, 0x2e).unwrap();
                assert_eq!(config.unlock().is_none(), fail);
                if !fail {
                    assert_eq!(io.options.get(), 0xa5);
                    assert!(config.exit_mode());
                }
            }
            assert_eq!(io.ldn.get(), 4);
            assert_eq!(io.options.get(), 0xb5);
            assert_eq!(io.last_port.get(), 0xaa);
        }
    }
    #[test]
    fn nuvoton_fan_counter_and_chip_ids_are_bounded() {
        assert_eq!(chip_profile(0xd802), Some(("NCT6799D", 7)));
        assert_eq!(chip_profile(0xd592), None);
        assert_eq!(fan_rpm((1125 >> 5) as u8, (1125 & 31) as u8), Some(1200.0));
        assert_eq!(fan_rpm(255, 255), Some(0.0));
        assert_eq!(fan_rpm(0, 0), Some(0.0));
        assert_eq!(fan_rpm(0, 1), None);
    }
}
