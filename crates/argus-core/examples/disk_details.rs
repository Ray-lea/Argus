//! `cargo run -p argus-core --example disk_details --release -- 0`
//! Prints one disk's detail API response. Serial numbers are redacted by default.
//! `--extended` exercises the fixed ATA/SAT/SCSI reads with the current rights.

use std::time::Instant;

use argus_core::hardware::disk_details;

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let index = args
        .first()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    let include_serial = args.iter().any(|arg| arg == "--include-serial");
    let at = Instant::now();
    let details = if args.iter().any(|arg| arg == "--extended") {
        disk_details::query_privileged(index)
    } else {
        disk_details::query(index)
    };
    let query_ms = at.elapsed().as_secs_f64() * 1000.0;
    let mut value = serde_json::to_value(&details).expect("serializable disk details");
    if !include_serial {
        for key in ["identity", "nvmeIdentity", "ataIdentity", "scsiIdentity"] {
            if let Some(identity) = value[key].as_object_mut()
                && identity.get("serialNumber").is_some_and(|v| !v.is_null())
            {
                identity.insert("serialNumber".into(), "[redacted]".into());
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "elevated": argus_core::privilege::is_elevated(),
            "queryMs": query_ms,
            "details": value,
        }))
        .expect("serializable probe")
    );
}
