//! Append-only system and process history, partitioned by UTC day. The live
//! sampler publishes ten minutes of unrounded system samples independently of
//! the five-second frames. Times and spans are milliseconds. System readings
//! average finite samples independently; CPU peak is their maximum. Process
//! working set, dedicated GPU memory, threads and handles are frame maxima.
//!
//! Sealed days use ARGH v4. The minute packets keep independent system and
//! process blocks, the observed intervals, and exact gap evidence. See the
//! minute module for the reader and atomic writer.
//!
//! # Version 3
//!
//! `YYYY-MM-DD.hist` begins with `ARGH\x03`. Each record is a u8 tag, u32
//! payload length and payload. Fixed-width numbers are little endian.
//!
//! * 3, channel definition: UTF-8 JSON pair [local index, HistoryChannel].
//!   A new index appends a channel; a repeated index changes its display label.
//! * 4, process identity: UTF-8 JSON pair [local index, {name,pid,created}].
//!   Creation time is Unix ms; nonpositive denotes unavailable.
//! * 5, session: kind u8 (1 start, 2 disabled, 3 exit), wall time, estimated
//!   boot time and unbiased interrupt time, each i64 ms. Day rollover repeats
//!   the session evidence. Boot estimates within two seconds are the same boot.
//! * 6, frame: timestamp and unbiased clock i64, span u32, process offset u64,
//!   process length u32, then channel values in index order. Byte quantities use
//!   f64, other units f32. NaN means a known channel with no reading. Negative
//!   infinity means the device was absent. Later catalog entries have no slot in
//!   earlier frames. The count and widths follow from the record and catalog.
//! * 7, quarter-hour system acceleration record: first, last and CPU peak frame
//!   i64, frame count u32 and highest peak f64, followed per channel by count
//!   u32 and sum, minimum and maximum f64. Partial groups fall back to frames.
//! * 8, channel presence: a bitmap in catalog order, low bit first, effective
//!   at the next frame. A new record is written on device arrival or removal.
//!
//! `YYYY-MM-DD.proc` contains byte ranges committed by tag 6. Each entry has a
//! ULEB128 identity index (bit 16 omits network, bit 17 omits threads and handles), flags u8, nonzero CPU/GPU f32 (bits 0/1), working
//! set, I/O rate, network rate, threads and handles, then optional disk read,
//! disk write, GPU memory and power (bits 2..5). Bit 6 stores nonzero power as
//! f32. A present zero power has no payload. Bit 7 adds a four-rate mask and
//! the divisors of fractional I/O, network, disk-read and disk-write means.
//! Their integer payload holds the numerator. Integer fields use ULEB128.
//! Working set and GPU memory encode page count plus one for aligned values,
//! or zero followed by the exact byte count. The writer flushes process bytes
//! before committing the main frame. Reopening truncates incomplete records
//! and unreferenced process tails to the last matching committed boundary.
//!
//! `YYYY-MM-DD.roll` accelerates process tables. Tag 1 with a u32 length
//! contains first timestamp, exclusive end and covered milliseconds (i64),
//! then pairs of identity index and mergeable Totals. These retain weighted
//! sums, maxima and observed durations for nullable fields and each trend.
//! Writers build summaries during regular frame appends. Queries combine whole
//! groups only inside one trend bin and decode detail at the edges. A missing
//! summary file or torn final summary falls back to the underlying frames. Process
//! summaries span up to five minutes.
//!
//! `.idx` contains optional append-only checkpoints of committed main-file byte
//! ranges. Each tag-1 packet begins with the start and end u32 offsets, followed
//! by copies of metadata and tag-9 runs. A run stores first/last clock stamps
//! (wall i64, unbiased i64, span u32), process end u64, and pairs of u32 UTC-day
//! milliseconds and main-file offsets. Gaps split runs. Queries load these compact
//! addresses and system summaries, then index the uncheckpointed main-file tail.
//! Missing or incomplete checkpoints fall back to the primary day file.
//!
//! NICs use the Windows interface GUID. Disks use the PnP interface path
//! or descriptor serial; GPUs use the PnP instance or PCI address. The last fallback
//! is an unambiguous model (and capacity for disks). Without a distinguishable
//! identity only the system total is recorded. Disk/GPU identity strings use
//! case-folded FNV-1a 64. Board fans use the controller channel, batteries are
//! aggregated across packs. All device data comes from the collected snapshot.
//!
//! Day files of other versions (the v1/v2 of releases before 0.6) are not
//! read, and a day begun in one starts over in v3. Legacy queries project v3
//! back into the original JSON structures.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ffi::OsStr;
use std::fs;
#[cfg(not(test))]
use std::fs::{File, metadata};
#[cfg(test)]
mod test_files;
use std::fs::OpenOptions;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(test)]
use test_files::{File, metadata};

use arc_swap::ArcSwap;
use serde::Serialize;

use crate::collector::Snapshot;

/// Length of one frame.
pub const FRAME_MS: i64 = 5_000;
mod events;
pub use events::{HistoryEvent, HistoryEvents, HistoryTop, ResourceKind, events};
mod options;
pub use options::*;
mod directory;
pub(crate) use directory::owned_directory;
pub use directory::{prepare_directory, select_directory, validate_directory};
mod maintenance;
pub use maintenance::{maintain, maintain_at, maintain_until};
mod migration;
pub use migration::move_records;
mod minute;
use minute::{MINUTE_MAGIC, MINUTE_MS, MINUTE_VERSION, Minute};

mod catalog;
mod checkpoints;
mod encoding;
mod files;
mod index;
mod legacy;
mod reading;
mod recorder;
mod series;
mod summaries;
mod table;
mod usage;
mod writer;

pub use catalog::{HistoryChannel, channels};
pub use files::clear;
pub use index::Gap;
pub use legacy::{HistoryMoment, HistoryPoint, HistoryProcess, legacy_series, moment};
pub use reading::{FrameProcess, HistoryFrame, ProgramCount, frame, range};
pub use recorder::HistoryRecorder;
pub use series::{Column, HistorySeries, series};
pub use table::{HistoryTable, TableProcess, table};
pub use usage::{Breakdown, HistoryEstimate, HistoryUsage, usage, usage_with};

// The parts share their items with one another and with the modules above
// (`use super::*`) through this module.
use catalog::*;
use checkpoints::*;
use encoding::*;
use files::*;
use index::*;
use reading::*;
use recorder::*;
use series::*;
use summaries::*;
use table::*;
use usage::*;
use writer::*;

/// The day file format written and read: v3 (frames, with process rows in
/// a `.proc` sidecar). Sealed days may be v4 (`minute::MINUTE_MAGIC`).
/// Files of other versions, such as the v1/v2 days of releases before 0.6,
/// are not read: as if nothing had been recorded that day.
const MAGIC: &[u8; 5] = b"ARGH\x03";
/// The version byte of `MAGIC`, which also tells a raw day (`Index::version`)
/// from a sealed one (`MINUTE_VERSION`).
const VERSION: u8 = MAGIC[4];
const DAY_MS: i64 = 86_400_000;
// A normal day holds 17,280 frames. Reject oversized files before allocating or repairing.
const MAX_FILE_BYTES: usize = 128 << 20;
const MAX_METADATA_BYTES: usize = 16 << 10;
const MAX_CHANNELS: usize = 512;
const MAX_INSTANCES: usize = 65_536;
const MAX_FRAME_ROWS: usize = MAX_INSTANCES;
// Ten minutes at the sampler's shortest supported cadence (250 ms), inclusive.
pub(crate) const MAX_RECENT: usize = 2401;
pub const RECENT_MS: i64 = 10 * 60_000;
const MAX_FRAMES: usize = 100_000;
const MAX_POINTS: usize = 4096;
const MAX_BUCKETS: usize = 100_000;
const MAX_QUERY_CELLS: usize = 2_000_000;

#[cfg(test)]
mod tests;
