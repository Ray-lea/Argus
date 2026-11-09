//! Presentation formats follow Windows regional settings independently of UI
//! language. Stored values, protocol fields, versions and identifiers stay invariant.
use std::sync::OnceLock;

struct Numbers {
    decimal: String,
    separator: String,
    groups: Vec<usize>,
}

fn numbers() -> &'static Numbers {
    static NUMBERS: OnceLock<Numbers> = OnceLock::new();
    NUMBERS.get_or_init(|| Numbers::load(None))
}

impl Numbers {
    fn load(locale: Option<&str>) -> Self {
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::Globalization::{
                GetLocaleInfoEx, LOCALE_NOUSEROVERRIDE, LOCALE_SDECIMAL, LOCALE_SGROUPING,
                LOCALE_STHOUSAND,
            };
            use windows::core::PCWSTR;
            let name = locale.map(|name| name.encode_utf16().chain(Some(0)).collect::<Vec<_>>());
            let locale = name
                .as_ref()
                .map_or(PCWSTR::null(), |name| PCWSTR(name.as_ptr()));
            let info = |kind, fallback: &str| {
                let mut buffer = [0u16; 80];
                let flags = if name.is_some() {
                    LOCALE_NOUSEROVERRIDE
                } else {
                    0
                };
                let size = unsafe { GetLocaleInfoEx(locale, kind | flags, Some(&mut buffer)) };
                if size > 0 {
                    String::from_utf16_lossy(&buffer[..size as usize - 1])
                } else {
                    fallback.to_owned()
                }
            };
            Numbers {
                decimal: info(LOCALE_SDECIMAL, "."),
                separator: info(LOCALE_STHOUSAND, ","),
                groups: info(LOCALE_SGROUPING, "3;0")
                    .split(';')
                    .filter_map(|v| v.parse().ok())
                    .collect(),
            }
        }
        #[cfg(not(target_os = "windows"))]
        Numbers {
            decimal: ".".into(),
            separator: ",".into(),
            groups: vec![3, 0],
        }
    }
}

/// For fixed-point UI figures only; never pass versions, paths or raw input.
pub(crate) fn numeric(raw: String) -> String {
    if numbers().decimal == "." {
        raw
    } else {
        raw.replace('.', &numbers().decimal)
    }
}

pub(crate) fn seconds(milliseconds: u64) -> String {
    numeric((milliseconds as f64 / 1000.).to_string())
}

macro_rules! number {
    ($($arg:tt)*) => { $crate::regional::numeric(format!($($arg)*)) };
}
pub(crate) use number;

pub(crate) fn count(value: f64) -> String {
    digits(&format!("{value:.0}"))
}

pub(crate) fn digits(raw: &str) -> String {
    let style = numbers();
    group(raw, &style.separator, &style.groups)
}

pub(crate) fn decimal(value: f64, digits: usize) -> String {
    let fixed = format!("{:.*}", digits, value.abs());
    let (whole, fraction) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let fraction = fraction.trim_end_matches('0');
    let style = numbers();
    let mut out = group(whole, &style.separator, &style.groups);
    if value < 0. && fixed.bytes().any(|c| c.is_ascii_digit() && c != b'0') {
        out.insert(0, '-');
    }
    if !fraction.is_empty() {
        out.push_str(&style.decimal);
        out.push_str(fraction);
    }
    out
}

fn group(raw: &str, separator: &str, sizes: &[usize]) -> String {
    let (sign, digits) = raw
        .strip_prefix('-')
        .map_or(("", raw), |digits| ("-", digits));
    if !digits.bytes().all(|c| c.is_ascii_digit()) || sizes.first().copied().unwrap_or(0) == 0 {
        return raw.to_owned();
    }
    let (mut at, mut index, mut size) = (digits.len(), 0, sizes[0]);
    let mut parts = Vec::new();
    while at > size {
        parts.push(&digits[at - size..at]);
        at -= size;
        if index + 1 == sizes.len() {
            break;
        }
        if sizes[index + 1] != 0 {
            index += 1;
            size = sizes[index];
        }
    }
    parts.push(&digits[..at]);
    parts.reverse();
    format!("{sign}{}", parts.join(separator))
}

/// Decimal input accepts the regional separator and an invariant point;
/// grouping is deliberately rejected because 1,000 could mean two values.
/// Full-width digits and punctuation are common in Japanese IME input.
pub(crate) fn parse_decimal(text: &str) -> Option<f64> {
    let raw: String = text
        .trim()
        .chars()
        .map(|c| match c {
            '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap(),
            '．' => '.',
            '，' => ',',
            '－' | '−' => '-',
            '＋' => '+',
            _ => c,
        })
        .collect();
    parse_with_separator(&raw, &numbers().decimal)
}

fn parse_with_separator(text: &str, separator: &str) -> Option<f64> {
    let raw = if separator != "." && text.contains(separator) {
        if text.contains('.') {
            return None;
        }
        text.replace(separator, ".")
    } else {
        text.to_owned()
    };
    raw.parse::<f64>().ok().filter(|value| value.is_finite())
}

/// Input is already converted with the offset valid at this instant, including DST.
pub(crate) fn date_time(value: time::OffsetDateTime) -> String {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::SYSTEMTIME;
        use windows::Win32::Globalization::{
            DATE_SHORTDATE, GetDateFormatEx, GetTimeFormatEx, TIME_FORMAT_FLAGS,
        };
        use windows::core::PCWSTR;
        let local = SYSTEMTIME {
            wYear: value.year() as u16,
            wMonth: value.month() as u16,
            wDay: value.day() as u16,
            wHour: value.hour() as u16,
            wMinute: value.minute() as u16,
            wSecond: value.second() as u16,
            ..Default::default()
        };
        let (mut date, mut clock) = ([0u16; 128], [0u16; 128]);
        let (d, t) = unsafe {
            (
                GetDateFormatEx(
                    PCWSTR::null(),
                    DATE_SHORTDATE,
                    Some(&local),
                    PCWSTR::null(),
                    Some(&mut date),
                    PCWSTR::null(),
                ),
                GetTimeFormatEx(
                    PCWSTR::null(),
                    TIME_FORMAT_FLAGS(0),
                    Some(&local),
                    PCWSTR::null(),
                    Some(&mut clock),
                ),
            )
        };
        if d > 0 && t > 0 {
            return format!(
                "{} {}",
                String::from_utf16_lossy(&date[..d as usize - 1]),
                String::from_utf16_lossy(&clock[..t as usize - 1])
            );
        }
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        value.year(),
        value.month() as u8,
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

pub(crate) fn offset_label(offset: time::UtcOffset) -> String {
    let seconds = offset.whole_seconds();
    let sign = if seconds < 0 { '-' } else { '+' };
    let seconds = seconds.unsigned_abs();
    format!("UTC{sign}{:02}:{:02}", seconds / 3600, seconds % 3600 / 60)
}

/// Clock boundaries are local; spacing on the UTC axis changes at DST transitions.
pub(crate) fn clock_ticks(from: i64, to: i64, step: i64) -> Vec<i64> {
    ticks_with_offset(from, to, step, |at| {
        time::OffsetDateTime::from_unix_timestamp(at.div_euclid(1000))
            .ok()
            .and_then(|value| time::UtcOffset::local_offset_at(value).ok())
            .map_or(0, |offset| i64::from(offset.whole_seconds()) * 1000)
    })
}

fn ticks_with_offset(
    from: i64,
    to: i64,
    step: i64,
    mut offset: impl FnMut(i64) -> i64,
) -> Vec<i64> {
    if step <= 0 || from > to {
        return vec![];
    }
    let mut ticks = Vec::new();
    let mut at = from;
    while at <= to && ticks.len() < 1024 {
        let before = offset(at);
        let Some(wall) = at.checked_add(before) else {
            break;
        };
        let Some(mut next) = wall
            .div_euclid(step)
            .checked_mul(step)
            .and_then(|wall| wall.checked_sub(before))
        else {
            break;
        };
        if next < at {
            let Some(later) = next.checked_add(step) else {
                break;
            };
            next = later;
        }
        let end = next.min(to);
        if offset(end) != before {
            // Find the transition, then align in its new offset. This retains
            // repeated autumn hours and skips nonexistent spring hours.
            let (mut low, mut high) = (at, end);
            while high - low > 1 {
                let middle = low + (high - low) / 2;
                if offset(middle) == before {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            at = high;
            continue;
        }
        if next > to {
            break;
        }
        ticks.push(next);
        let Some(later) = next.checked_add(1) else {
            break;
        };
        at = later;
    }
    ticks
}

#[cfg(test)]
mod tests {
    use super::{
        Numbers, group, offset_label, parse_decimal, parse_with_separator, ticks_with_offset,
    };
    #[test]
    fn grouping_respects_signs_spaces_and_indian_regions() {
        assert_eq!(group("-1234567", ".", &[3, 0]), "-1.234.567");
        assert_eq!(
            group("1234567", "\u{202f}", &[3, 0]),
            "1\u{202f}234\u{202f}567"
        );
        assert_eq!(group("12345678", ",", &[3, 2, 0]), "1,23,45,678");
        assert_eq!(group("12345678", ",", &[3]), "12345,678");
        assert_eq!(group("12345678", "", &[0]), "12345678");
    }
    #[test]
    fn decimal_input_never_drops_a_separator() {
        assert_eq!(parse_with_separator("12,5", ","), Some(12.5));
        assert_eq!(parse_with_separator("12.5", ","), Some(12.5));
        for invalid in ["1.234,5", "1,234.5", "12 5", "1,2,3", "NaN", "inf"] {
            assert_eq!(parse_with_separator(invalid, ","), None);
        }
        assert_eq!(parse_decimal("１２．５"), Some(12.5));
    }

    #[test]
    fn history_ticks_follow_spring_autumn_and_half_hour_transitions() {
        const HOUR: i64 = 3_600_000;
        const DAY: i64 = 24 * HOUR;
        let spring = |at| if at < 2 * HOUR { 0 } else { HOUR };
        assert_eq!(
            ticks_with_offset(0, 3 * DAY, DAY, spring),
            [0, DAY - HOUR, 2 * DAY - HOUR, 3 * DAY - HOUR]
        );
        assert_eq!(
            ticks_with_offset(0, 6 * HOUR, 3 * HOUR, spring),
            [0, 2 * HOUR, 5 * HOUR]
        );
        let autumn = |at| if at < 2 * HOUR { HOUR } else { 0 };
        let ticks = ticks_with_offset(-HOUR, 5 * HOUR, HOUR, autumn);
        assert_eq!(
            ticks,
            [-HOUR, 0, HOUR, 2 * HOUR, 3 * HOUR, 4 * HOUR, 5 * HOUR]
        );
        assert_eq!(ticks[2] + autumn(ticks[2]), ticks[3] + autumn(ticks[3]));
        assert_eq!(ticks_with_offset(-HOUR, DAY, DAY, autumn), [-HOUR, DAY]);
        assert_eq!(
            ticks_with_offset(0, 5 * HOUR, HOUR, |at| if at < 2 * HOUR {
                0
            } else {
                HOUR / 2
            }),
            [0, HOUR, 5 * HOUR / 2, 7 * HOUR / 2, 9 * HOUR / 2]
        );
        assert_eq!(
            ticks_with_offset(0, 2 * HOUR, HOUR, |_| 23 * HOUR / 4),
            [HOUR / 4, 5 * HOUR / 4]
        );
    }

    #[test]
    fn offsets_keep_quarter_hours_and_negative_zero_hours() {
        assert_eq!(
            offset_label(time::UtcOffset::from_hms(5, 45, 0).unwrap()),
            "UTC+05:45"
        );
        assert_eq!(
            offset_label(time::UtcOffset::from_hms(0, -30, 0).unwrap()),
            "UTC-00:30"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn regional_numbers_match_windows_without_changing_user_settings() {
        use windows::Win32::Globalization::{GetNumberFormatEx, LOCALE_NOUSEROVERRIDE};
        use windows::core::{PCWSTR, w};
        let locales = argus_core::i18n::Locale::ALL
            .into_iter()
            .map(|locale| locale.tag())
            .chain(["de-CH", "de-AT", "fr-CA", "es-MX", "en-GB", "hi-IN"]);
        for locale in locales {
            let name: Vec<_> = locale.encode_utf16().chain(Some(0)).collect();
            let mut native = [0u16; 128];
            let length = unsafe {
                GetNumberFormatEx(
                    PCWSTR(name.as_ptr()),
                    LOCALE_NOUSEROVERRIDE,
                    w!("1234567.25"),
                    None,
                    Some(&mut native),
                )
            };
            assert!(length > 0, "{locale}");
            let style = Numbers::load(Some(locale));
            let ours = format!(
                "{}{}25",
                group("1234567", &style.separator, &style.groups),
                style.decimal
            );
            assert_eq!(
                ours,
                String::from_utf16_lossy(&native[..length as usize - 1]),
                "{}",
                locale
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn european_summer_time_ticks_match_windows_zone_rules() {
        use windows::Win32::Foundation::SYSTEMTIME;
        use windows::Win32::System::Time::{
            DYNAMIC_TIME_ZONE_INFORMATION, EnumDynamicTimeZoneInformation,
            SystemTimeToTzSpecificLocalTimeEx,
        };
        let mut zone = DYNAMIC_TIME_ZONE_INFORMATION::default();
        let mut found = false;
        for index in 0..1024 {
            if unsafe { EnumDynamicTimeZoneInformation(index, &mut zone) } != 0 {
                break;
            }
            let name =
                String::from_utf16_lossy(zone.TimeZoneKeyName.split(|c| *c == 0).next().unwrap());
            if name == "W. Europe Standard Time" {
                found = true;
                break;
            }
        }
        assert!(found);
        let offset = |at: i64| {
            let utc = time::OffsetDateTime::from_unix_timestamp(at.div_euclid(1000)).unwrap();
            let input = SYSTEMTIME {
                wYear: utc.year() as u16,
                wMonth: utc.month() as u16,
                wDay: utc.day() as u16,
                wHour: utc.hour() as u16,
                wMinute: utc.minute() as u16,
                wSecond: utc.second() as u16,
                ..Default::default()
            };
            let mut output = SYSTEMTIME::default();
            unsafe { SystemTimeToTzSpecificLocalTimeEx(Some(&zone), &input, &mut output) }.unwrap();
            let wall = time::Date::from_calendar_date(
                output.wYear as i32,
                time::Month::try_from(output.wMonth as u8).unwrap(),
                output.wDay as u8,
            )
            .unwrap()
            .with_hms(
                output.wHour as u8,
                output.wMinute as u8,
                output.wSecond as u8,
            )
            .unwrap()
            .assume_utc();
            (wall.unix_timestamp() - utc.unix_timestamp()) * 1000
        };
        const HOUR: i64 = 3_600_000;
        for (month, day, hours) in [(time::Month::March, 29, 23), (time::Month::October, 25, 25)] {
            let from = time::Date::from_calendar_date(2026, month, day)
                .unwrap()
                .midnight()
                .assume_utc()
                .unix_timestamp()
                * 1000
                - 2 * HOUR;
            let ticks = ticks_with_offset(from, from + 3 * 24 * HOUR, 24 * HOUR, offset);
            assert!(
                ticks
                    .windows(2)
                    .any(|pair| pair[1] - pair[0] == hours * HOUR)
            );
            assert!(
                ticks
                    .iter()
                    .all(|at| (at + offset(*at)).rem_euclid(24 * HOUR) == 0)
            );
        }
    }
}
