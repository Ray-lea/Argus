//! Prints device telemetry (CPU, sensors, memory, disks, NICs, GPUs, batteries) to verify the collectors.
//! `cargo run -p argus-core --example hardware --release`

use std::time::{Duration, Instant};

use argus_core::{Collector, MetaStore};

fn main() {
    let t = Instant::now();
    let mut c = Collector::new(MetaStore::new());
    println!("init: {:?}", t.elapsed());
    std::thread::sleep(Duration::from_millis(1000));
    let t = Instant::now();
    let s = c.sample();
    println!("sample: {:?}\n", t.elapsed());

    let hw = &s.hardware;
    println!("CPU {:?}", hw.cpu);
    println!(
        "  now {:?} MHz, usage {:.1}% (kernel {:.1}%)",
        hw.cpu_mhz, s.system.cpu, s.system.cpu_kernel
    );
    println!("  per-core MHz {:?}", hw.core_mhz);
    let kinds: Vec<_> = hw
        .cpu
        .threads
        .iter()
        .map(|t| if t.efficient { "E" } else { "P" })
        .collect();
    println!("  threads {}", kinds.join(""));
    println!("  thermal {:?}", hw.thermal);
    println!("  power {:?}", hw.power);
    println!("MEM {:?}", hw.memory);
    for d in &hw.disks {
        println!(
            "DISK {} {:?} {} GB bus={} ssd={:?} vols={:?} | active {:.1}% r {} KB/s w {} KB/s resp {:.2} ms temp {:?} (warn {:?})",
            d.info.index,
            d.info.model,
            d.info.size / 1_000_000_000,
            d.info.bus,
            d.info.ssd,
            d.info.volumes,
            d.active,
            d.read / 1024,
            d.write / 1024,
            d.response_ms,
            d.temperature,
            d.temperature_warning
        );
    }
    for n in &hw.nics {
        println!(
            "NIC {:?} ({}) kind={} link={} Mbps mac={:?} v4={:?} v6={:?} | rx {} KB/s tx {} KB/s",
            n.info.name,
            n.info.description,
            n.info.kind,
            n.info.link_speed / 1_000_000,
            n.info.mac,
            n.info.ipv4,
            n.info.ipv6,
            n.rx / 1024,
            n.tx / 1024
        );
    }
    for g in &hw.gpus {
        println!(
            "GPU {:?} id={} dedicated {}/{} MB shared {}/{} MB temp {:?} (max {:?}) fan {:?} power {:?}% board {:?}/{:?} W clock {:?}/{:?} MHz {:?} mV mem {:?} MHz",
            g.info.name,
            g.info.id,
            g.dedicated_used >> 20,
            g.info.dedicated_total >> 20,
            g.shared_used >> 20,
            g.info.shared_total >> 20,
            g.temperature,
            g.info.temperature_max,
            g.fan_rpm,
            g.power,
            g.board_power,
            g.board_power_limit,
            g.clock_mhz,
            g.clock_max_mhz,
            g.voltage_mv,
            g.memory_clock_mhz
        );
        for (name, u) in g.info.engines.iter().zip(&g.engines) {
            println!("    {name:<20} {u:.1}%");
        }
        println!("    => util {:.1}%", g.util);
    }
    for b in &hw.batteries {
        println!("BAT {:?}", b.info);
        println!(
            "    {:.1}% {} plugged={} capacity {:?} mWh rate {:?} mW voltage {:?} mV remaining {:?} s temp {:?}",
            b.percent,
            b.state,
            b.plugged_in,
            b.capacity,
            b.rate,
            b.voltage,
            b.remaining,
            b.temperature
        );
    }
    let t = Instant::now();
    for _ in 0..10 {
        c.sample();
    }
    println!(
        "
sample x10: {:?} each",
        t.elapsed() / 10
    );
}
