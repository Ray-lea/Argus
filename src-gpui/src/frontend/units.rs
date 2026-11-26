//! Byte quantities in the unit system chosen in the settings (`byteUnit`).
//!
//! The traditional system counts in 1024s under the SI symbols, as Windows
//! writes sizes; IEC counts in 1024s under KiB, MiB…; SI counts in 1000s.
//! Bit rates stay decimal whichever is chosen. The formats take the system
//! explicitly; `current` is the process-wide choice the interface shows.
//! Tests pass a system rather than applying one: they run in parallel.

use std::borrow::Cow;
use std::sync::atomic::{AtomicU8, Ordering};

use super::prefs::Prefs;

/// The preference that holds the choice.
pub(in crate::frontend) const PREFERENCE: &str = "byteUnit";

/// Powers of the multiples, for `ByteUnit::multiple` and the symbols.
pub(in crate::frontend) const KILO: usize = 1;
pub(in crate::frontend) const MEGA: usize = 2;
pub(in crate::frontend) const GIGA: usize = 3;
pub(in crate::frontend) const TERA: usize = 4;

const SYMBOLS: [&str; 7] = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
const IEC_SYMBOLS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
const RATES: [&str; 7] = ["B/s", "KB/s", "MB/s", "GB/s", "TB/s", "PB/s", "EB/s"];
const IEC_RATES: [&str; 7] = ["B/s", "KiB/s", "MiB/s", "GiB/s", "TiB/s", "PiB/s", "EiB/s"];

static CURRENT: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(in crate::frontend) enum ByteUnit {
    /// 1 MB = 1024 KB.
    #[default]
    Traditional,
    /// 1 MiB = 1024 KiB.
    Iec,
    /// 1 MB = 1000 KB.
    Si,
}

impl ByteUnit {
    /// The choices in the order the setting lists them.
    pub(in crate::frontend) const ALL: [Self; 3] = [Self::Iec, Self::Si, Self::Traditional];

    pub(in crate::frontend) fn key(self) -> &'static str {
        match self {
            Self::Traditional => "traditional",
            Self::Iec => "iec",
            Self::Si => "si",
        }
    }

    /// A saved choice; anything else is the default.
    pub(in crate::frontend) fn parse(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|unit| unit.key() == key)
            .unwrap_or_default()
    }

    pub(in crate::frontend) fn load(prefs: &Prefs) -> Self {
        Self::parse(&prefs.get::<String>(PREFERENCE, String::new()))
    }

    /// The system the interface shows.
    pub(in crate::frontend) fn current() -> Self {
        match CURRENT.load(Ordering::Relaxed) {
            1 => Self::Iec,
            2 => Self::Si,
            _ => Self::Traditional,
        }
    }

    /// Makes this the system the interface shows; the caller redraws.
    pub(in crate::frontend) fn apply(self) {
        let index = match self {
            Self::Traditional => 0,
            Self::Iec => 1,
            Self::Si => 2,
        };
        CURRENT.store(index, Ordering::Relaxed);
    }

    /// The size of a multiple: `multiple(MEGA)` is a megabyte.
    pub(in crate::frontend) fn multiple(self, power: usize) -> f64 {
        let base: f64 = if self == Self::Si { 1000. } else { 1024. };
        base.powi(power as i32)
    }

    pub(in crate::frontend) fn symbol(self, power: usize) -> &'static str {
        if self == Self::Iec {
            IEC_SYMBOLS[power]
        } else {
            SYMBOLS[power]
        }
    }

    pub(in crate::frontend) fn rate_symbol(self, power: usize) -> &'static str {
        if self == Self::Iec {
            IEC_RATES[power]
        } else {
            RATES[power]
        }
    }

    /// `bytes` in a multiple, to `digits` decimals: "4.0 MB".
    pub(in crate::frontend) fn figure(self, bytes: f64, power: usize, digits: usize) -> String {
        crate::regional::number!(
            "{:.digits$} {}",
            bytes / self.multiple(power),
            self.symbol(power)
        )
    }

    /// Gigabytes to a tenth, without the symbol: "12.3".
    pub(in crate::frontend) fn gigabytes(self, bytes: f64) -> String {
        crate::regional::number!("{:.1}", bytes / self.multiple(GIGA))
    }

    /// Memory: tenths of a gigabyte from ten up, hundredths from one, else
    /// tenths of a megabyte: "12.3 GB", "3.45 GB", "512.0 MB".
    pub(in crate::frontend) fn memory(self, bytes: f64) -> String {
        let gigabytes = bytes / self.multiple(GIGA);
        if gigabytes >= 10. {
            self.figure(bytes, GIGA, 1)
        } else if gigabytes >= 1. {
            self.figure(bytes, GIGA, 2)
        } else {
            self.figure(bytes, MEGA, 1)
        }
    }

    /// A size, a bare "0" when empty: whole bytes and kilobytes, then tenths
    /// up to terabytes: "512 B", "12 KB", "3.4 MB".
    pub(in crate::frontend) fn size(self, bytes: f64) -> String {
        if bytes == 0. {
            return "0".into();
        }
        let mut power = 0;
        while power < TERA && bytes >= self.multiple(power + 1) {
            power += 1;
        }
        self.figure(bytes, power, if power <= KILO { 0 } else { 1 })
    }

    /// A rate, a bare "0" when idle (`rate_shown` otherwise).
    pub(in crate::frontend) fn rate(self, bytes: f64) -> String {
        if bytes < 1. {
            "0".into()
        } else {
            self.rate_shown(bytes)
        }
    }

    /// A rate with its unit even when idle: whole kilobytes, then tenths of
    /// a megabyte and hundredths of a gigabyte: "0 KB/s", "312 KB/s", "1.2 MB/s".
    pub(in crate::frontend) fn rate_shown(self, bytes: f64) -> String {
        if bytes < 1. {
            format!("0 {}", self.rate_symbol(KILO))
        } else if bytes < self.multiple(MEGA) {
            format!(
                "{} {}",
                (bytes / self.multiple(KILO)).round().max(1.),
                self.rate_symbol(KILO)
            )
        } else if bytes < self.multiple(GIGA) {
            crate::regional::number!(
                "{:.1} {}",
                bytes / self.multiple(MEGA),
                self.rate_symbol(MEGA)
            )
        } else {
            crate::regional::number!(
                "{:.2} {}",
                bytes / self.multiple(GIGA),
                self.rate_symbol(GIGA)
            )
        }
    }

    /// A drive's capacity: tenths of a terabyte, else whole gigabytes.
    pub(in crate::frontend) fn capacity(self, bytes: f64) -> String {
        if bytes >= self.multiple(TERA) {
            self.figure(bytes, TERA, 1)
        } else {
            self.rounded(bytes, GIGA)
        }
    }

    /// A processor cache: tenths of a megabyte, else whole kilobytes.
    pub(in crate::frontend) fn cache(self, bytes: f64) -> String {
        if bytes >= self.multiple(MEGA) {
            self.figure(bytes, MEGA, 1)
        } else {
            self.rounded(bytes, KILO)
        }
    }

    /// Whole multiples, halves rounded up: "513 GB".
    fn rounded(self, bytes: f64, power: usize) -> String {
        format!(
            "{} {}",
            (bytes / self.multiple(power)).round(),
            self.symbol(power)
        )
    }

    /// A sector or cluster, exactly: "4 KB" where it is a whole number of
    /// kilobytes, "4096 B" otherwise, as in decimal units.
    pub(in crate::frontend) fn block(self, bytes: u64) -> String {
        let kilo = self.multiple(KILO) as u64;
        if bytes >= kilo && bytes.is_multiple_of(kilo) {
            format!("{} {}", bytes / kilo, self.symbol(KILO))
        } else {
            format!("{bytes} B")
        }
    }

    /// A sample written in the traditional symbols, in this system's:
    /// "888.8 MB/s" becomes "888.8 MiB/s" in IEC units.
    pub(in crate::frontend) fn relabel(self, sample: &'static str) -> Cow<'static, str> {
        if self != Self::Iec {
            return Cow::Borrowed(sample);
        }
        let mut out = sample.to_owned();
        for prefix in ["K", "M", "G", "T", "P", "E"] {
            out = out.replace(&format!(" {prefix}B"), &format!(" {prefix}iB"));
        }
        Cow::Owned(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{ByteUnit, GIGA, KILO, MEGA, TERA};
    use crate::regional::numeric;

    const GIB: f64 = 1_073_741_824.;
    const MIB: f64 = 1_048_576.;

    #[test]
    fn choices_round_trip_and_unknown_keys_keep_the_default() {
        for unit in ByteUnit::ALL {
            assert_eq!(ByteUnit::parse(unit.key()), unit);
        }
        assert_eq!(ByteUnit::parse(""), ByteUnit::Traditional);
        assert_eq!(ByteUnit::parse("binary"), ByteUnit::Traditional);
    }

    #[test]
    fn multiples_and_symbols_follow_the_system() {
        assert_eq!(ByteUnit::Traditional.multiple(MEGA), MIB);
        assert_eq!(ByteUnit::Iec.multiple(GIGA), GIB);
        assert_eq!(ByteUnit::Si.multiple(TERA), 1e12);
        assert_eq!(ByteUnit::Traditional.symbol(KILO), "KB");
        assert_eq!(ByteUnit::Iec.symbol(KILO), "KiB");
        assert_eq!(ByteUnit::Si.symbol(KILO), "KB");
        assert_eq!(ByteUnit::Iec.rate_symbol(GIGA), "GiB/s");
    }

    #[test]
    fn traditional_formats_keep_their_binary_values() {
        let unit = ByteUnit::Traditional;
        assert_eq!(unit.memory(16. * GIB), numeric("16.0 GB".into()));
        assert_eq!(unit.memory(3.45 * GIB), numeric("3.45 GB".into()));
        assert_eq!(unit.memory(512. * MIB), numeric("512.0 MB".into()));
        assert_eq!(unit.size(0.), "0");
        assert_eq!(unit.size(512.), "512 B");
        assert_eq!(unit.size(12. * 1024.), "12 KB");
        assert_eq!(unit.size(2. * GIB * 1024.), numeric("2.0 TB".into()));
        assert_eq!(unit.rate(0.5), "0");
        assert_eq!(unit.rate_shown(0.), "0 KB/s");
        assert_eq!(unit.rate_shown(10.), "1 KB/s");
        assert_eq!(unit.rate_shown(1_000_000.), "977 KB/s");
        assert_eq!(unit.rate_shown(3. * MIB), numeric("3.0 MB/s".into()));
        assert_eq!(unit.rate_shown(2. * GIB), numeric("2.00 GB/s".into()));
        assert_eq!(unit.capacity(2e12), numeric("1.8 TB".into()));
        assert_eq!(unit.capacity(512. * GIB), "512 GB");
        assert_eq!(unit.cache(12. * MIB), numeric("12.0 MB".into()));
        assert_eq!(unit.cache(640. * 1024.), "640 KB");
        assert_eq!(unit.block(4096), "4 KB");
        assert_eq!(unit.block(512), "512 B");
    }

    #[test]
    fn iec_formats_count_in_binary_under_iec_symbols() {
        let unit = ByteUnit::Iec;
        assert_eq!(unit.memory(16. * GIB), numeric("16.0 GiB".into()));
        assert_eq!(unit.size(12. * 1024.), "12 KiB");
        assert_eq!(unit.rate_shown(0.), "0 KiB/s");
        assert_eq!(unit.rate_shown(3. * MIB), numeric("3.0 MiB/s".into()));
        assert_eq!(unit.capacity(2e12), numeric("1.8 TiB".into()));
        assert_eq!(unit.block(4096), "4 KiB");
    }

    #[test]
    fn si_formats_count_in_thousands() {
        let unit = ByteUnit::Si;
        // Memory sold as 16 GiB is 17.2 decimal gigabytes.
        assert_eq!(unit.memory(16. * GIB), numeric("17.2 GB".into()));
        assert_eq!(unit.memory(999_000_000.), numeric("999.0 MB".into()));
        assert_eq!(unit.size(999.), "999 B");
        assert_eq!(unit.size(1000.), "1 KB");
        assert_eq!(unit.size(1_500_000.), numeric("1.5 MB".into()));
        assert_eq!(unit.rate_shown(1_000_000.), numeric("1.0 MB/s".into()));
        assert_eq!(unit.rate_shown(999_000.), "999 KB/s");
        // A byte rate is an eighth of the bit rate, exactly.
        assert_eq!(unit.rate_shown(12_500_000.), numeric("12.5 MB/s".into()));
        assert_eq!(unit.capacity(2e12), numeric("2.0 TB".into()));
        assert_eq!(unit.capacity(512e9), "512 GB");
        // Binary block sizes are shown exactly rather than as 4.1 KB.
        assert_eq!(unit.block(4096), "4096 B");
        assert_eq!(unit.block(512), "512 B");
    }

    #[test]
    fn samples_take_the_symbols_of_the_system() {
        assert_eq!(ByteUnit::Traditional.relabel("888.8 MB/s"), "888.8 MB/s");
        assert_eq!(ByteUnit::Si.relabel("8888 KB"), "8888 KB");
        assert_eq!(ByteUnit::Iec.relabel("888.8 MB/s"), "888.8 MiB/s");
        assert_eq!(ByteUnit::Iec.relabel("8888 KB"), "8888 KiB");
        assert_eq!(ByteUnit::Iec.relabel("888.8 Mbps"), "888.8 Mbps");
    }
}
