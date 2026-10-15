use crate::schema::{TimestampPrecision, TimestampValue};

fn digits(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}
pub fn offset_minutes(value: &str) -> Option<i16> {
    if value == "Z" {
        return Some(0);
    }
    let b = value.as_bytes();
    if b.len() != 6 || !matches!(b[0], b'+' | b'-') || b[3] != b':' {
        return None;
    }
    let h = digits(value.get(1..3)?)?;
    let m = digits(value.get(4..6)?)?;
    if h > 23 || m > 59 {
        return None;
    }
    Some((if b[0] == b'-' { -1 } else { 1 }) * (h * 60 + m) as i16)
}

/// ISO 8601 / EXIF and PDF dates. Missing zones and partial dates remain unresolved.
pub fn parse_timestamp(original: &str, companion_offset: Option<&str>) -> Option<TimestampValue> {
    let input = original.trim().trim_end_matches('\0');
    let expanded;
    let input = if let Some(pdf) = input.strip_prefix("D:") {
        let n = pdf.bytes().take_while(u8::is_ascii_digit).count();
        if !matches!(n, 4 | 6 | 8 | 12 | 14) {
            return None;
        }
        let mut s = pdf.get(..4)?.to_owned();
        if n >= 6 {
            s.push('-');
            s.push_str(pdf.get(4..6)?);
        }
        if n >= 8 {
            s.push('-');
            s.push_str(pdf.get(6..8)?);
        }
        if n >= 12 {
            s.push('T');
            s.push_str(pdf.get(8..10)?);
            s.push(':');
            s.push_str(pdf.get(10..12)?);
        }
        if n >= 14 {
            s.push(':');
            s.push_str(pdf.get(12..14)?);
        }
        let zone = pdf.get(n..)?.replace('\'', "");
        if zone.len() == 5 && matches!(zone.as_bytes()[0], b'+' | b'-') {
            s.push_str(zone.get(..3)?);
            s.push(':');
            s.push_str(zone.get(3..)?);
        } else {
            s.push_str(&zone);
        }
        expanded = s;
        expanded.as_str()
    } else {
        input
    };
    let b = input.as_bytes();
    let y = digits(input.get(..4)?)?;
    let mut mo = 1;
    let mut day = 1;
    let mut h = 0;
    let mut m = 0;
    let mut s = 0;
    let mut at = 4;
    let mut precision = TimestampPrecision::Year;
    if matches!(b.get(at), Some(b'-' | b':')) {
        let separator = b[at];
        mo = digits(input.get(5..7)?)?;
        at = 7;
        precision = TimestampPrecision::Month;
        if b.get(at) == Some(&separator) {
            day = digits(input.get(8..10)?)?;
            at = 10;
            precision = TimestampPrecision::Day;
        }
    }
    if matches!(b.get(at), Some(b'T' | b' ')) {
        if at != 10 || b.get(13) != Some(&b':') {
            return None;
        }
        h = digits(input.get(11..13)?)?;
        m = digits(input.get(14..16)?)?;
        at = 16;
        precision = TimestampPrecision::Minute;
        if b.get(at) == Some(&b':') {
            s = digits(input.get(17..19)?)?;
            at = 19;
            precision = TimestampPrecision::Second;
        }
    }
    let leap = y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if y == 0
        || !(1..=12).contains(&mo)
        || day == 0
        || day > days[(mo - 1) as usize]
        || h > 23
        || m > 59
        || s > 60
    {
        return None;
    }
    let mut ns = None;
    if b.get(at) == Some(&b'.') {
        if precision != TimestampPrecision::Second {
            return None;
        }
        at += 1;
        let begin = at;
        while b.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        let fraction = input.get(begin..at)?;
        if fraction.len() > 9 {
            return None;
        }
        ns = Some(digits(fraction)? * 10u32.pow((9 - fraction.len()) as u32));
        precision = TimestampPrecision::FractionalSecond;
    }
    let zone = input.get(at..)?;
    let explicit = if zone.is_empty() {
        None
    } else {
        Some(offset_minutes(zone)?)
    };
    let external = companion_offset.map(offset_minutes).transpose_option()?;
    if explicit.zip(external).is_some_and(|(a, b)| a != b) {
        return None;
    }
    let offset = if zone == "-00:00" || (zone.is_empty() && companion_offset == Some("-00:00")) {
        None
    } else {
        explicit.or(external)
    };
    let prev = i64::from(y) - 1;
    let epoch_days = 365 * prev + prev / 4 - prev / 100
        + prev / 400
        + i64::from(days[..(mo - 1) as usize].iter().sum::<u32>())
        + i64::from(day)
        - 1
        - 719_162;
    let absolute = matches!(
        precision,
        TimestampPrecision::Second
            | TimestampPrecision::FractionalSecond
            | TimestampPrecision::Minute
    ) && s != 60;
    Some(TimestampValue {
        original: original.into(),
        unix_seconds: offset
            .filter(|_| absolute)
            .map(|o| epoch_days * 86400 + i64::from(h * 3600 + m * 60 + s) - i64::from(o) * 60),
        nanosecond: ns,
        utc_offset_minutes: offset,
        precision,
    })
}
trait TransposeOption<T> {
    fn transpose_option(self) -> Option<Option<T>>;
}
impl<T> TransposeOption<T> for Option<Option<T>> {
    fn transpose_option(self) -> Option<Option<T>> {
        match self {
            None => Some(None),
            Some(Some(v)) => Some(Some(v)),
            Some(None) => None,
        }
    }
}

pub fn from_unix_nanoseconds(value: i128) -> Option<TimestampValue> {
    Some(TimestampValue {
        original: value.to_string(),
        unix_seconds: Some(i64::try_from(value.div_euclid(1_000_000_000)).ok()?),
        nanosecond: Some(value.rem_euclid(1_000_000_000) as u32),
        utc_offset_minutes: Some(0),
        precision: TimestampPrecision::FractionalSecond,
    })
}
