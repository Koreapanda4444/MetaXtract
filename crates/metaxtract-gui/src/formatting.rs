pub fn bytes(value: u64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut number = value as f64;
    let mut unit = 0;
    while number >= 1024.0 && unit < units.len() - 1 {
        number /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value} B")
    } else {
        format!("{number:.2} {}", units[unit])
    }
}
pub fn unix_seconds(seconds: i128) -> String {
    let days = seconds.div_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i128::from(month <= 2);
    if !(0..=9999).contains(&year) {
        return format!("{seconds} s UTC");
    }
    let time = seconds.rem_euclid(86400);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        time / 3600,
        time / 60 % 60,
        time % 60
    )
}
pub fn modified(ns: Option<&str>) -> String {
    ns.and_then(|s| s.parse::<i128>().ok())
        .map(|n| unix_seconds(n.div_euclid(1_000_000_000)))
        .unwrap_or_else(|| "—".into())
}

pub fn raw(value: &metaxtract_app::schema::RawValue, unit: Option<&str>) -> String {
    use metaxtract_app::schema::RawValue;
    let number = match value {
        RawValue::Unsigned(n) => Some(*n as f64),
        RawValue::Signed(n) => Some(*n as f64),
        RawValue::Float(n) => Some(*n),
        RawValue::Rational(n) if n.denominator != 0 => {
            Some(n.numerator as f64 / n.denominator as f64)
        }
        _ => None,
    };
    if let (Some(number), Some(unit)) = (number.filter(|n| n.is_finite()), unit) {
        if matches!(unit, "bit/s" | "bits/s" | "bps") {
            return if number.abs() >= 1_000_000.0 {
                format!("{:.3} Mbps", number / 1_000_000.0)
            } else {
                format!("{number} bps")
            };
        }
        if matches!(unit, "bytes" | "B" | "byte") {
            if let RawValue::Unsigned(n) = value {
                return bytes(*n);
            }
        }
        return format!("{number} {unit}");
    }
    match value {
        RawValue::Text(t) => t
            .text
            .chars()
            .take(200)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect(),
        RawValue::Bytes(v) => bytes(v.len() as u64),
        RawValue::Sequence(v) => crate::textf!("{} values", "{}개 값", v.len()),
        RawValue::Structure(v) => crate::textf!("{} fields", "{}개 필드", v.len()),
        RawValue::Null => "—".into(),
        RawValue::Bool(v) => v.to_string(),
        RawValue::Signed(v) => v.to_string(),
        RawValue::Unsigned(v) => v.to_string(),
        RawValue::Float(v) => v.to_string(),
        RawValue::Rational(v) => format!("{}/{}", v.numerator, v.denominator),
    }
}
pub fn normalized(value: &metaxtract_app::schema::NormalizedValue) -> String {
    use metaxtract_app::schema::NormalizedValue;
    match value {
        NormalizedValue::Timestamp(t) => {
            let zone = t
                .utc_offset_minutes
                .map(|minutes| {
                    format!(
                        "UTC{}{:02}:{:02}",
                        if minutes < 0 { "-" } else { "+" },
                        minutes.unsigned_abs() / 60,
                        minutes.unsigned_abs() % 60
                    )
                })
                .unwrap_or_else(|| crate::i18n::tr("Time zone unknown").into());
            format!("{} · {zone}", t.original)
        }
        NormalizedValue::GeoPoint(point) => format!(
            "{:.7}°, {:.7}°{}",
            point.latitude,
            point.longitude,
            point
                .altitude_meters
                .map(|v| format!(" · {v} m"))
                .unwrap_or_default()
        ),
        NormalizedValue::DurationNanoseconds(n) => format!("{:.3} s", *n as f64 / 1_000_000_000.0),
        NormalizedValue::Sequence(v) => crate::textf!("{} values", "{}개 값", v.len()),
        NormalizedValue::Text(v) => v
            .chars()
            .take(200)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect(),
        NormalizedValue::Null => "—".into(),
        NormalizedValue::Bool(v) => v.to_string(),
        NormalizedValue::Signed(v) => v.to_string(),
        NormalizedValue::Unsigned(v) => v.to_string(),
        NormalizedValue::Number(v) => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_epoch_and_fractional_hour_time_zones_keep_their_meaning() {
        assert_eq!(unix_seconds(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(modified(Some("-1")), "1969-12-31 23:59:59 UTC");
        let timestamp = metaxtract_app::schema::TimestampValue {
            original: "2026-01-01T00:00:00-00:30".into(),
            unix_seconds: Some(1767227400),
            nanosecond: None,
            utc_offset_minutes: Some(-30),
            precision: metaxtract_app::schema::TimestampPrecision::Second,
        };
        assert!(
            normalized(&metaxtract_app::schema::NormalizedValue::Timestamp(
                timestamp
            ))
            .contains("UTC-00:30")
        );
        assert_eq!(bytes(1024), "1.00 KiB");
    }
}
