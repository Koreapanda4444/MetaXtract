use std::collections::{BTreeMap, BTreeSet};

use metaxtract_schema::{
    AgreementLevel, CanonicalKey, ClaimConflict, ClaimId, ClaimResolution, Confidence,
    ConfidenceBasis, ExtractionMethod, MetadataObservation, NormalizedClaim, NormalizedValue,
    ObservationId, RawValue, TimestampPrecision, TimestampValue,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageIssueKind {
    InvalidDimension,
    InvalidOrientation,
    InvalidTimestamp,
    UnknownTimezone,
    UnresolvedLeapSecond,
    UnlinkedHeifMetadata,
    ColorSpaceMismatch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageIssue {
    pub kind: ImageIssueKind,
    pub observations: Vec<ObservationId>,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageNormalizationError {
    MixedSources,
    DuplicateObservationId,
    AnalysisLimitExceeded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageAnalysisLimits {
    pub max_observations: usize,
    pub max_alternatives_per_key: usize,
    pub max_claim_links: usize,
}

impl Default for ImageAnalysisLimits {
    fn default() -> Self {
        Self {
            max_observations: 65_536,
            max_alternatives_per_key: 64,
            max_claim_links: 1_000_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageAnalysis {
    pub claims: Vec<NormalizedClaim>,
    pub conflicts: Vec<ClaimConflict>,
    pub issues: Vec<ImageIssue>,
    pub display_dimensions: Option<(u64, u64)>,
}

fn value(observation: &MetadataObservation) -> &RawValue {
    observation
        .interpreted_value
        .as_ref()
        .unwrap_or(&observation.raw_value)
}

fn text(value: &RawValue) -> Option<&str> {
    if let RawValue::Text(value) = value {
        Some(&value.text)
    } else {
        None
    }
}

fn unsigned(value: &RawValue) -> Option<u64> {
    match value {
        RawValue::Unsigned(value) => Some(*value),
        RawValue::Signed(value) => (*value).try_into().ok(),
        RawValue::Text(value) => value.text.trim().parse().ok(),
        _ => None,
    }
}

fn name(observation: &MetadataObservation) -> &str {
    observation.tag.name.as_deref().unwrap_or("")
}

fn issue(
    issues: &mut Vec<ImageIssue>,
    kind: ImageIssueKind,
    ids: Vec<ObservationId>,
    detail: &str,
) {
    issues.push(ImageIssue {
        kind,
        observations: ids,
        detail: detail.into(),
    });
}

fn family(observation: &MetadataObservation) -> String {
    observation.provenance.parser.library.as_ref().map_or_else(
        || {
            format!(
                "builtin:{}",
                observation
                    .provenance
                    .parser
                    .id
                    .split('.')
                    .next()
                    .unwrap_or("")
            )
        },
        |library| format!("library:{}", library.name),
    )
}

struct Candidate<'a> {
    value: NormalizedValue,
    observations: Vec<&'a MetadataObservation>,
}

fn equivalent(first: &NormalizedValue, second: &NormalizedValue) -> bool {
    if let (NormalizedValue::Timestamp(a), NormalizedValue::Timestamp(b)) = (first, second) {
        if let (Some(a_time), Some(b_time)) = (a.unix_seconds, b.unix_seconds) {
            return a_time == b_time && a.nanosecond == b.nanosecond && a.precision == b.precision;
        }
    }
    first == second
}

fn incompatible(first: &NormalizedValue, second: &NormalizedValue) -> bool {
    if let (NormalizedValue::Timestamp(a), NormalizedValue::Timestamp(b)) = (first, second) {
        return match (a.unix_seconds, b.unix_seconds) {
            (Some(a_time), Some(b_time)) if a_time != b_time => true,
            (Some(_), Some(_)) => a.nanosecond.zip(b.nanosecond).is_some_and(|(a, b)| a != b),
            _ => false,
        };
    }
    !equivalent(first, second)
}

fn add<'a>(
    claims: &mut BTreeMap<String, Vec<Candidate<'a>>>,
    key: String,
    value: NormalizedValue,
    observation: &'a MetadataObservation,
    limits: &ImageAnalysisLimits,
) -> Result<(), ImageNormalizationError> {
    let alternatives = claims.entry(key).or_default();
    if let Some(existing) = alternatives
        .iter_mut()
        .find(|candidate| equivalent(&candidate.value, &value))
    {
        existing.observations.push(observation);
    } else {
        if alternatives.len() >= limits.max_alternatives_per_key {
            return Err(ImageNormalizationError::AnalysisLimitExceeded);
        }
        alternatives.push(Candidate {
            value,
            observations: vec![observation],
        });
    }
    Ok(())
}

fn scope(namespace: &str) -> String {
    if let Some((before, suffix)) = namespace.split_once(".item") {
        let id = suffix.split('.').next().unwrap_or("");
        return format!("{before}.item{id}");
    }
    namespace.to_owned()
}

fn dimension_key(
    namespace: &str,
    key: &str,
    chunks: &BTreeMap<String, String>,
    primary: &BTreeSet<String>,
) -> Option<String> {
    if key == "sensor_width" || key == "sensor_height" {
        return Some(format!("image.sensor.{}", key.strip_prefix("sensor_")?));
    }
    if key == "thumbnail_width" || key == "thumbnail_height" {
        return Some(format!(
            "image.thumbnail.{}",
            key.strip_prefix("thumbnail_")?
        ));
    }
    let axis = match key {
        "width" | "pixel_x_dimension" => "width",
        "height" | "pixel_y_dimension" => "height",
        _ => return None,
    };
    if !namespace.starts_with("image.") {
        return None;
    }
    if namespace.starts_with("image.heif.") && !primary.contains(&scope(namespace)) {
        return None;
    }
    if namespace.contains(".maker_note") || namespace.contains(".subifd") {
        return None;
    }
    if namespace.starts_with("image.raw.original") {
        return Some(format!("image.sensor.{axis}"));
    }
    if namespace.contains(".exif.ifd1") {
        return Some(format!("image.thumbnail.{axis}"));
    }
    if let Some((_, suffix)) = namespace.split_once(".frame") {
        return Some(format!(
            "image.frames.frame{}.{axis}",
            suffix.split('.').next()?.parse::<u32>().ok()?
        ));
    }
    if let Some((_, suffix)) = namespace.split_once(".entry") {
        return Some(format!(
            "image.variants.entry{}.{axis}",
            suffix.split('.').next()?.parse::<u32>().ok()?
        ));
    }
    if chunks.get(namespace).is_some_and(|kind| kind == "ANMF") {
        return Some(format!(
            "image.frames.chunk{}.{axis}",
            namespace
                .rsplit('.')
                .next()?
                .strip_prefix("chunk")?
                .parse::<u32>()
                .ok()?
        ));
    }
    if namespace.starts_with("image.tiff.ifd") && !namespace.starts_with("image.tiff.ifd0") {
        return Some(format!(
            "image.pages.ifd{}.{axis}",
            namespace
                .split('.')
                .nth(2)?
                .strip_prefix("ifd")?
                .parse::<u32>()
                .ok()?
        ));
    }
    Some(format!("image.{axis}"))
}

fn digits(value: &str, begin: usize, end: usize) -> Option<u32> {
    let bytes = value.as_bytes().get(begin..end)?;
    if !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

fn role_field(
    namespace: &str,
    field: &str,
    chunks: &BTreeMap<String, String>,
    primary: &BTreeSet<String>,
) -> Option<String> {
    let dimension = dimension_key(namespace, "width", chunks, primary)?;
    Some(format!("{}.{}", dimension.strip_suffix(".width")?, field))
}

fn parse_offset(value: &str) -> Option<i16> {
    if value == "Z" {
        return Some(0);
    }
    let bytes = value.as_bytes();
    if bytes.len() != 6 || !matches!(bytes[0], b'+' | b'-') || bytes[3] != b':' {
        return None;
    }
    let hours = digits(value, 1, 3)?;
    let minutes = digits(value, 4, 6)?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    let total = (hours * 60 + minutes) as i16;
    Some(if bytes[0] == b'-' { -total } else { total })
}

fn timestamp(
    original: &str,
    external_offset: Option<&str>,
    external_fraction: Option<&str>,
) -> Option<TimestampValue> {
    let original = original.trim_end_matches('\0').trim();
    let bytes = original.as_bytes();
    let partial = match bytes.len() {
        4 => Some((
            format!("{original}-01-01T00:00:00"),
            TimestampPrecision::Year,
        )),
        7 if bytes[4] == b'-' => {
            Some((format!("{original}-01T00:00:00"), TimestampPrecision::Month))
        }
        10 if bytes[4] == b'-' && bytes[7] == b'-' => {
            Some((format!("{original}T00:00:00"), TimestampPrecision::Day))
        }
        length if length >= 16 && bytes.get(16) != Some(&b':') => Some((
            format!("{}:00{}", original.get(..16)?, original.get(16..)?),
            TimestampPrecision::Minute,
        )),
        _ => None,
    };
    if let Some((complete, precision)) = partial {
        if external_fraction.is_some() {
            return None;
        }
        let mut parsed = timestamp(&complete, external_offset, None)?;
        parsed.original = original.to_owned();
        parsed.precision = precision;
        parsed.unix_seconds = None;
        return Some(parsed);
    }
    if bytes.len() < 19
        || !matches!(bytes[4], b':' | b'-')
        || bytes[7] != bytes[4]
        || !matches!(bytes[10], b' ' | b'T')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let year = digits(original, 0, 4)?;
    let month = digits(original, 5, 7)?;
    let day = digits(original, 8, 10)?;
    let hour = digits(original, 11, 13)?;
    let minute = digits(original, 14, 16)?;
    let second = digits(original, 17, 19)?;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let month_days = [
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
    if year == 0
        || !(1..=12).contains(&month)
        || day == 0
        || day > month_days[(month - 1) as usize]
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut at = 19;
    let mut fraction = external_fraction;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        let begin = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        fraction = original.get(begin..at);
    }
    let nanosecond = if let Some(fraction) = fraction {
        if fraction.is_empty()
            || fraction.len() > 9
            || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        Some(fraction.parse::<u32>().ok()? * 10_u32.pow((9 - fraction.len()) as u32))
    } else {
        None
    };
    let embedded = original.get(at..)?;
    let offset = if embedded.is_empty() {
        match external_offset {
            None | Some("-00:00") => None,
            Some(value) => Some(parse_offset(value)?),
        }
    } else {
        let parsed = parse_offset(embedded)?;
        if external_offset
            .is_some_and(|value| value != "-00:00" && parse_offset(value) != Some(parsed))
        {
            return None;
        }
        if embedded == "-00:00" {
            None
        } else {
            Some(parsed)
        }
    };
    let previous = i64::from(year - 1);
    let days = 365 * previous + previous / 4 - previous / 100
        + previous / 400
        + i64::from(month_days[..(month - 1) as usize].iter().sum::<u32>())
        + i64::from(day - 1)
        - 719_162;
    let unix_seconds = offset.filter(|_| second != 60).map(|offset| {
        days * 86_400 + i64::from(hour * 3600 + minute * 60 + second) - i64::from(offset) * 60
    });
    Some(TimestampValue {
        original: original.to_owned(),
        unix_seconds,
        nanosecond,
        utc_offset_minutes: offset,
        precision: if nanosecond.is_some() {
            TimestampPrecision::FractionalSecond
        } else {
            TimestampPrecision::Second
        },
    })
}

type Matrix = [i8; 4];
const ORIENTATIONS: [Matrix; 8] = [
    [1, 0, 0, 1],
    [-1, 0, 0, 1],
    [-1, 0, 0, -1],
    [1, 0, 0, -1],
    [0, 1, 1, 0],
    [0, -1, 1, 0],
    [0, -1, -1, 0],
    [0, 1, -1, 0],
];

fn compose(a: Matrix, b: Matrix) -> Matrix {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
    ]
}

pub fn reconcile_image_metadata(
    observations: &[MetadataObservation],
) -> Result<ImageAnalysis, ImageNormalizationError> {
    reconcile_image_metadata_with_limits(observations, ImageAnalysisLimits::default())
}

pub fn reconcile_image_metadata_with_limits(
    observations: &[MetadataObservation],
    limits: ImageAnalysisLimits,
) -> Result<ImageAnalysis, ImageNormalizationError> {
    if observations.len() > limits.max_observations {
        return Err(ImageNormalizationError::AnalysisLimitExceeded);
    }
    let mut ids = BTreeSet::new();
    for observation in observations {
        if !ids.insert(observation.id) {
            return Err(ImageNormalizationError::DuplicateObservationId);
        }
        if observations
            .first()
            .is_some_and(|first| first.provenance.source != observation.provenance.source)
        {
            return Err(ImageNormalizationError::MixedSources);
        }
    }
    let mut raw_claims: BTreeMap<String, Vec<Candidate<'_>>> = BTreeMap::new();
    let mut issues = Vec::new();
    let mut chunks = BTreeMap::new();
    let mut primary = BTreeSet::new();
    let mut companion_index: BTreeMap<&str, BTreeMap<&str, Vec<&MetadataObservation>>> =
        BTreeMap::new();
    for observation in observations {
        if matches!(
            name(observation),
            "offset_time"
                | "offset_time_original"
                | "offset_time_digitized"
                | "subsec_time_original"
        ) && text(value(observation)).is_some()
        {
            companion_index
                .entry(observation.tag.namespace.as_str())
                .or_default()
                .entry(name(observation))
                .or_default()
                .push(observation);
        }
        if name(observation) == "type"
            && let Some(kind) = text(value(observation))
        {
            chunks.insert(observation.tag.namespace.clone(), kind.to_owned());
        }
        if name(observation) == "is_primary" && unsigned(value(observation)) == Some(1) {
            primary.insert(observation.tag.namespace.clone());
        }
    }
    let mut transforms: BTreeMap<String, (Matrix, Vec<&MetadataObservation>)> = BTreeMap::new();
    let mut invalid_main_orientation = false;
    for observation in observations {
        let key = name(observation);
        let namespace = &observation.tag.namespace;
        if !namespace.starts_with("image.") {
            continue;
        }
        let is_xmp = key.starts_with('{');
        let exif_xmp = key.strip_prefix("{http://ns.adobe.com/exif/1.0/}");
        let tiff_xmp = key.strip_prefix("{http://ns.adobe.com/tiff/1.0/}");
        let xmp = key.strip_prefix("{http://ns.adobe.com/xap/1.0/}");
        let mapped = match (exif_xmp, tiff_xmp) {
            (Some("PixelXDimension"), _) | (_, Some("ImageWidth")) => "pixel_x_dimension",
            (Some("PixelYDimension"), _) | (_, Some("ImageLength")) => "pixel_y_dimension",
            (_, Some("Orientation")) => "orientation",
            (Some("DateTimeOriginal"), _) => "datetime_original",
            (Some("DateTimeDigitized"), _) => "datetime_digitized",
            (Some("ColorSpace"), _) => "exif_color_space",
            _ if xmp == Some("CreateDate") => "created",
            _ if xmp == Some("ModifyDate") || tiff_xmp == Some("DateTime") => "timestamp",
            _ => key,
        };
        if namespace.starts_with("image.heif.")
            && namespace.contains(".metadata")
            && !primary.contains(&scope(namespace))
        {
            if matches!(
                mapped,
                "pixel_x_dimension"
                    | "pixel_y_dimension"
                    | "orientation"
                    | "datetime_original"
                    | "timestamp"
            ) {
                issue(
                    &mut issues,
                    ImageIssueKind::UnlinkedHeifMetadata,
                    vec![observation.id],
                    "Metadata item association with the primary image is not established",
                );
            }
            continue;
        }
        if let Some(key) = dimension_key(namespace, mapped, &chunks, &primary) {
            if let Some(number) = unsigned(value(observation)).filter(|number| *number != 0) {
                add(
                    &mut raw_claims,
                    key,
                    NormalizedValue::Unsigned(number),
                    observation,
                    &limits,
                )?;
            } else {
                issue(
                    &mut issues,
                    ImageIssueKind::InvalidDimension,
                    vec![observation.id],
                    "Image dimension is not a positive integer",
                );
            }
        }
        if mapped == "orientation"
            && let Some(dimension) = dimension_key(namespace, "width", &chunks, &primary)
        {
            let orientation_key = if namespace.starts_with("image.raw.original") {
                "image.orientation".into()
            } else {
                format!(
                    "{}.orientation",
                    dimension
                        .strip_suffix(".width")
                        .expect("dimension key suffix")
                )
            };
            if let Some(number) =
                unsigned(value(observation)).filter(|number| (1..=8).contains(number))
            {
                add(
                    &mut raw_claims,
                    orientation_key,
                    NormalizedValue::Unsigned(number),
                    observation,
                    &limits,
                )?;
            } else {
                invalid_main_orientation |= orientation_key == "image.orientation";
                issue(
                    &mut issues,
                    ImageIssueKind::InvalidOrientation,
                    vec![observation.id],
                    "EXIF orientation must be between 1 and 8",
                );
            }
        }
        if matches!(key, "rotation_quarters" | "mirror_axis") && primary.contains(&scope(namespace))
        {
            let number = unsigned(value(observation));
            let matrix = if key == "rotation_quarters" {
                number
                    .filter(|n| *n <= 3)
                    .map(|n| ORIENTATIONS[[0, 7, 2, 5][n as usize]])
            } else {
                invalid_main_orientation = true;
                number
                    .filter(|n| *n <= 1)
                    .map(|n| ORIENTATIONS[if n == 0 { 1 } else { 3 }])
            };
            if let Some(matrix) = matrix {
                let current = transforms
                    .entry(scope(namespace))
                    .or_insert((ORIENTATIONS[0], Vec::new()));
                current.0 = compose(matrix, current.0);
                current.1.push(observation);
            } else {
                issue(
                    &mut issues,
                    ImageIssueKind::InvalidOrientation,
                    vec![observation.id],
                    "HEIF transform contains reserved bits or an invalid axis",
                );
            }
        }
        let color = match mapped {
            "photometric_interpretation" => {
                unsigned(value(observation)).and_then(|value| match value {
                    0 | 1 => Some("GRAY"),
                    2 => Some("RGB"),
                    3 => Some("PALETTE"),
                    5 => Some("CMYK"),
                    6 => Some("YCbCr"),
                    8..=10 => Some("Lab"),
                    32803 => Some("CFA"),
                    _ => None,
                })
            }
            "color_type" if namespace.starts_with("image.png") => unsigned(value(observation))
                .and_then(|value| match value {
                    0 | 4 => Some("GRAY"),
                    2 | 6 => Some("RGB"),
                    3 => Some("PALETTE"),
                    _ => None,
                }),
            _ => None,
        };
        if let Some(color) = color
            && let Some(key) = role_field(namespace, "stored_color_model", &chunks, &primary)
        {
            add(
                &mut raw_claims,
                key,
                NormalizedValue::Text(color.into()),
                observation,
                &limits,
            )?;
        }
        if mapped == "color_space"
            && let Some(color) = text(value(observation))
            && let Some(key) = role_field(namespace, "profile_color_model", &chunks, &primary)
        {
            let color = match color.trim() {
                "YCbr" => "YCbCr",
                value => value,
            };
            add(
                &mut raw_claims,
                key,
                NormalizedValue::Text(color.into()),
                observation,
                &limits,
            )?;
        }
        if (mapped == "color_space" || mapped == "exif_color_space")
            && unsigned(value(observation)) == Some(1)
            && let Some(key) = role_field(namespace, "declared_color_profile", &chunks, &primary)
        {
            add(
                &mut raw_claims,
                key,
                NormalizedValue::Text("sRGB".into()),
                observation,
                &limits,
            )?;
        }
        let time_key = match mapped {
            "datetime_original" => Some("time.capture"),
            "datetime_digitized" => Some("time.digitized"),
            "timestamp" => Some("time.modified"),
            "created" => Some("time.created"),
            _ => None,
        };
        if let Some(time_key) = time_key
            && let Some(original) = text(value(observation))
        {
            let time_key = dimension_key(namespace, "width", &chunks, &primary)
                .and_then(|dimension| dimension.strip_suffix(".width").map(str::to_owned))
                .filter(|role| role != "image" && role != "image.sensor")
                .map_or_else(|| time_key.to_owned(), |role| format!("{role}.{time_key}"));
            let offset_key = match mapped {
                "datetime_original" => "offset_time_original",
                "datetime_digitized" => "offset_time_digitized",
                _ => "offset_time",
            };
            let companion = |key: &str| {
                companion_index
                    .get(namespace.as_str())
                    .and_then(|items| items.get(key))
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
            };
            let offsets = if is_xmp {
                &[][..]
            } else {
                companion(offset_key)
            };
            let fractions = if is_xmp || mapped != "datetime_original" {
                &[][..]
            } else {
                companion("subsec_time_original")
            };
            if offsets
                .iter()
                .map(|item| text(value(item)))
                .collect::<BTreeSet<_>>()
                .len()
                > 1
                || fractions
                    .iter()
                    .map(|item| text(value(item)))
                    .collect::<BTreeSet<_>>()
                    .len()
                    > 1
            {
                issue(
                    &mut issues,
                    ImageIssueKind::InvalidTimestamp,
                    std::iter::once(observation.id)
                        .chain(offsets.iter().chain(fractions).map(|item| item.id))
                        .collect(),
                    "Timestamp has contradictory timezone or fractional-second companions",
                );
                continue;
            }
            let offset = offsets.first().and_then(|item| text(value(item)));
            let fraction = fractions.first().and_then(|item| text(value(item)));
            if let Some(parsed) = timestamp(original, offset, fraction) {
                if parsed.utc_offset_minutes.is_none() {
                    issue(
                        &mut issues,
                        ImageIssueKind::UnknownTimezone,
                        vec![observation.id],
                        "Timestamp has no explicit UTC offset; no absolute instant is inferred",
                    );
                }
                if parsed.utc_offset_minutes.is_some()
                    && parsed.unix_seconds.is_none()
                    && parsed.precision == TimestampPrecision::Second
                {
                    issue(
                        &mut issues,
                        ImageIssueKind::UnresolvedLeapSecond,
                        vec![observation.id],
                        "Leap second retained without projecting it to ordinary Unix seconds",
                    );
                }
                add(
                    &mut raw_claims,
                    time_key.clone(),
                    NormalizedValue::Timestamp(parsed),
                    observation,
                    &limits,
                )?;
                if let Some(last) = raw_claims.get_mut(&time_key).and_then(|items| {
                    items
                        .iter_mut()
                        .find(|item| item.observations.iter().any(|old| old.id == observation.id))
                }) {
                    last.observations
                        .extend(offsets.iter().chain(fractions).copied());
                }
            } else {
                issue(
                    &mut issues,
                    ImageIssueKind::InvalidTimestamp,
                    vec![observation.id],
                    "Timestamp or its timezone/fraction companion is invalid",
                );
            }
        }
    }
    for (_, (matrix, support)) in transforms {
        if let Some(index) = ORIENTATIONS.iter().position(|value| *value == matrix) {
            for observation in support {
                add(
                    &mut raw_claims,
                    "image.orientation".into(),
                    NormalizedValue::Unsigned(index as u64 + 1),
                    observation,
                    &limits,
                )?;
            }
        }
    }
    for (key, stored) in raw_claims
        .iter()
        .filter(|(key, _)| key.ends_with(".stored_color_model"))
    {
        let Some(profile) =
            raw_claims.get(&key.replace(".stored_color_model", ".profile_color_model"))
        else {
            continue;
        };
        for a in stored {
            for b in profile {
                if let (NormalizedValue::Text(a_text), NormalizedValue::Text(b_text)) =
                    (&a.value, &b.value)
                {
                    if !matches!(
                        b_text.as_str(),
                        "RGB" | "GRAY" | "CMYK" | "Lab" | "XYZ" | "YCbCr"
                    ) {
                        continue;
                    }
                    let compatible = a_text == b_text
                        || (matches!(a_text.as_str(), "PALETTE" | "YCbCr" | "CFA")
                            && b_text == "RGB");
                    if !compatible {
                        issue(
                            &mut issues,
                            ImageIssueKind::ColorSpaceMismatch,
                            a.observations
                                .iter()
                                .chain(&b.observations)
                                .map(|item| item.id)
                                .collect(),
                            "Stored color model is inconsistent with the declared ICC input space",
                        );
                    }
                }
            }
        }
    }
    let unique_number = |key: &str| {
        raw_claims
            .get(key)
            .filter(|values| values.len() == 1)
            .and_then(|values| match values[0].value {
                NormalizedValue::Unsigned(value) => Some(value),
                _ => None,
            })
    };
    let display_dimensions = if let (Some(width), Some(height)) =
        (unique_number("image.width"), unique_number("image.height"))
    {
        let orientation = unique_number("image.orientation");
        if invalid_main_orientation
            || (raw_claims.contains_key("image.orientation") && orientation.is_none())
        {
            None
        } else {
            Some(if orientation.is_some_and(|value| value >= 5) {
                (height, width)
            } else {
                (width, height)
            })
        }
    } else {
        None
    };
    let mut claims = Vec::new();
    let mut conflicts = Vec::new();
    let mut links_used = 0usize;
    for (key, alternatives) in raw_claims {
        let mut claim_ids = Vec::new();
        for alternative in &alternatives {
            let disputed = alternatives
                .iter()
                .any(|other| incompatible(&alternative.value, &other.value));
            let id = ClaimId::new(claims.len() as u64 + 1);
            if disputed {
                claim_ids.push(id);
            }
            let families = alternative
                .observations
                .iter()
                .filter(|item| {
                    !matches!(
                        name(item),
                        "offset_time"
                            | "offset_time_original"
                            | "offset_time_digitized"
                            | "subsec_time_original"
                    )
                })
                .map(|item| family(item))
                .collect::<BTreeSet<_>>();
            let structural = alternative.observations.iter().any(|item| {
                key.starts_with("image.")
                    && item.provenance.method == ExtractionMethod::Structure
                    && !item.tag.namespace.contains(".metadata")
                    && !item.tag.namespace.contains(".exif")
                    && !item.tag.namespace.starts_with("image.tiff")
                    && !item.tag.namespace.starts_with("image.raw")
                    && !name(item).starts_with('{')
            });
            let mut basis = vec![if structural {
                ConfidenceBasis::StructuralValidation
            } else {
                ConfidenceBasis::DirectTag
            }];
            if families.len() > 1 {
                basis.push(ConfidenceBasis::IndependentParserAgreement);
            }
            let supporting_parsers = alternative
                .observations
                .iter()
                .map(|item| item.provenance.parser.id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let others = alternatives
                .iter()
                .filter(|other| incompatible(&alternative.value, &other.value))
                .flat_map(|other| &other.observations)
                .collect::<Vec<_>>();
            links_used = links_used
                .checked_add(alternative.observations.len())
                .and_then(|count| count.checked_add(others.len()))
                .ok_or(ImageNormalizationError::AnalysisLimitExceeded)?;
            if links_used > limits.max_claim_links {
                return Err(ImageNormalizationError::AnalysisLimitExceeded);
            }
            claims.push(NormalizedClaim {
                id,
                key: CanonicalKey::parse(key.clone()).expect("internal canonical key"),
                value: alternative.value.clone(),
                supporting_observations: alternative
                    .observations
                    .iter()
                    .map(|item| item.id)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                contradicting_observations: others.iter().map(|item| item.id).collect(),
                supporting_parsers,
                dissenting_parsers: others
                    .iter()
                    .map(|item| item.provenance.parser.id.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                confidence: Confidence::new(
                    if disputed {
                        450
                    } else if families.len() > 1 {
                        900
                    } else if structural {
                        850
                    } else {
                        650
                    },
                    basis,
                )
                .expect("internal confidence"),
                agreement: if disputed {
                    AgreementLevel::Disputed
                } else if families.len() > 1 {
                    AgreementLevel::Corroborated
                } else {
                    AgreementLevel::SingleParser
                },
                resolution: if disputed {
                    ClaimResolution::Unresolved
                } else {
                    ClaimResolution::Accepted
                },
            });
        }
        if !claim_ids.is_empty() {
            conflicts.push(ClaimConflict {
                key: CanonicalKey::parse(key).expect("internal canonical key"),
                alternatives: claim_ids,
                reason: "Image metadata sources report incompatible values".into(),
            });
        }
    }
    Ok(ImageAnalysis {
        claims,
        conflicts,
        issues,
        display_dimensions,
    })
}

#[cfg(test)]
mod checks {
    use super::*;
    use metaxtract_native::{
        BoundedInput, InputLimits, NativeObservation, ParserLimits, extract_parser,
    };
    use metaxtract_schema::{
        EvidencePath, LibraryDescriptor, ParserDescriptor, RelativePath, TextValue,
    };

    fn source() -> EvidencePath {
        EvidencePath::root(RelativePath::parse("evidence.tif").unwrap())
    }
    fn parser() -> ParserDescriptor {
        ParserDescriptor {
            id: "images.metadata".into(),
            name: "image metadata".into(),
            version: "builtin".into(),
            library: None,
        }
    }
    fn row(id: u64, namespace: &str, key: &str, value: RawValue) -> MetadataObservation {
        NativeObservation {
            namespace: namespace.into(),
            key: key.into(),
            tag_code: None,
            value,
            encoded: Vec::new(),
            source_range: None,
            method: ExtractionMethod::Structure,
            stream_id: None,
        }
        .to_canonical(ObservationId::new(id), source(), parser())
    }
    fn string(value: &str) -> RawValue {
        RawValue::Text(TextValue {
            text: value.into(),
            original_encoding: Some("ASCII".into()),
            language: None,
            encoded_bytes: None,
            lossless: true,
        })
    }

    #[test]
    fn native_dimensions_xmp_and_orientation_keep_conflicts_and_original_evidence() {
        let xmp = b"<x xmlns:e='http://ns.adobe.com/exif/1.0/' e:PixelXDimension='99'/>";
        let mut bytes = b"II*\0\x08\0\0\0\x04\0".to_vec();
        for (tag, kind, count, content) in [
            (256_u16, 4_u16, 1_u32, 3_u32),
            (257, 4, 1, 2),
            (274, 3, 1, 6),
            (700, 1, xmp.len() as u32, 62),
        ] {
            bytes.extend(tag.to_le_bytes());
            bytes.extend(kind.to_le_bytes());
            bytes.extend(count.to_le_bytes());
            bytes.extend(content.to_le_bytes());
        }
        bytes.extend([0; 4]);
        bytes.extend(xmp);
        let input = BoundedInput::from_bytes(bytes, InputLimits::default()).unwrap();
        let extracted = extract_parser(&input, "images.metadata", ParserLimits::default()).unwrap();
        assert_eq!(extracted.error, None);
        let rows = extracted
            .observations
            .iter()
            .enumerate()
            .map(|(i, value)| {
                value.to_canonical(ObservationId::new(i as u64 + 1), source(), parser())
            })
            .collect::<Vec<_>>();
        let original = rows.clone();
        let analysis = reconcile_image_metadata(&rows).unwrap();
        assert_eq!(analysis.display_dimensions, None);
        let conflict = analysis
            .conflicts
            .iter()
            .find(|conflict| conflict.key.as_str() == "image.width")
            .unwrap();
        assert_eq!(conflict.alternatives.len(), 2);
        assert!(
            analysis
                .claims
                .iter()
                .filter(|claim| claim.key.as_str() == "image.width")
                .all(|claim| claim.resolution == ClaimResolution::Unresolved)
        );
        assert_eq!(rows, original);
        let clean = rows
            .into_iter()
            .filter(|row| !name(row).contains("PixelXDimension"))
            .collect::<Vec<_>>();
        assert_eq!(
            reconcile_image_metadata(&clean).unwrap().display_dimensions,
            Some((2, 3))
        );
    }

    #[test]
    fn heif_transforms_and_thumbnail_dimensions_have_distinct_scopes() {
        let rows = vec![
            row(1, "image.heif.item1", "is_primary", RawValue::Unsigned(1)),
            row(2, "image.heif.item1.ispe", "width", RawValue::Unsigned(3)),
            row(3, "image.heif.item1.ispe", "height", RawValue::Unsigned(2)),
            row(
                4,
                "image.heif.item1.irot",
                "rotation_quarters",
                RawValue::Unsigned(3),
            ),
            row(5, "image.heif.item2.ispe", "width", RawValue::Unsigned(10)),
            row(6, "image.heif.item2.ispe", "height", RawValue::Unsigned(20)),
            row(
                7,
                "image.heif.item2.metadata.exif.ifd0",
                "orientation",
                RawValue::Unsigned(8),
            ),
        ];
        let result = reconcile_image_metadata(&rows).unwrap();
        assert_eq!(result.display_dimensions, Some((2, 3)));
        assert!(result.conflicts.is_empty());
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.kind == ImageIssueKind::UnlinkedHeifMetadata)
        );
        assert_eq!(
            result
                .claims
                .iter()
                .find(|claim| claim.key.as_str() == "image.orientation")
                .unwrap()
                .value,
            NormalizedValue::Unsigned(6)
        );
    }

    #[test]
    fn timestamps_never_infer_a_timezone_and_retain_companion_provenance() {
        assert_eq!(
            timestamp("2026", None, None).unwrap().precision,
            TimestampPrecision::Year
        );
        assert_eq!(
            timestamp("2024-02-29", None, None).unwrap().precision,
            TimestampPrecision::Day
        );
        assert!(timestamp("2025-02-29", None, None).is_none());
        assert_eq!(
            timestamp("1970-01-01T00:00Z", None, None)
                .unwrap()
                .unix_seconds,
            None
        );
        assert_eq!(
            timestamp("1970-01-01T00:00:00Z", None, None)
                .unwrap()
                .unix_seconds,
            Some(0)
        );
        assert_eq!(
            timestamp("1970:01:01 09:00:00", Some("+09:00"), Some("125"))
                .unwrap()
                .nanosecond,
            Some(125_000_000)
        );
        assert_eq!(
            timestamp("1970:01:01 09:00:00", Some("+09:00"), None)
                .unwrap()
                .unix_seconds,
            Some(0)
        );
        assert_eq!(
            timestamp("2026-10-11T12:34:56-00:00", None, None)
                .unwrap()
                .unix_seconds,
            None
        );
        assert_eq!(
            timestamp("2026:10:11 12:34:56", None, None)
                .unwrap()
                .utc_offset_minutes,
            None
        );
        assert!(timestamp("2025:02:29 12:00:00", Some("+00:00"), None).is_none());
        assert!(timestamp("2000:02:29 12:00:00", Some("+00:00"), None).is_some());
        assert_eq!(
            timestamp("2016-12-31T23:59:60Z", None, None)
                .unwrap()
                .unix_seconds,
            None
        );
        assert!(timestamp("2026:10:11 12:34:56", Some("+24:00"), None).is_none());
        let rows = vec![
            row(
                1,
                "image.tiff.ifd0.exif0",
                "datetime_original",
                string("1970:01:01 09:00:00"),
            ),
            row(
                2,
                "image.tiff.ifd0.exif0",
                "offset_time_original",
                string("+09:00"),
            ),
            row(
                3,
                "image.tiff.ifd0.exif0",
                "subsec_time_original",
                string("125"),
            ),
        ];
        let result = reconcile_image_metadata(&rows).unwrap();
        assert_eq!(
            result.claims[0].supporting_observations,
            vec![
                ObservationId::new(1),
                ObservationId::new(2),
                ObservationId::new(3)
            ]
        );
        let mut conflicting = rows;
        conflicting.push(row(
            4,
            "image.tiff.ifd0.exif0",
            "offset_time_original",
            string("+00:00"),
        ));
        let result = reconcile_image_metadata(&conflicting).unwrap();
        assert!(result.claims.is_empty());
        assert_eq!(result.issues[0].kind, ImageIssueKind::InvalidTimestamp);
    }

    #[test]
    fn correlated_parsers_do_not_inflate_agreement_and_invalid_evidence_is_rejected() {
        let a = row(1, "image.png", "width", RawValue::Unsigned(3));
        let mut b = a.clone();
        b.id = ObservationId::new(2);
        b.provenance.parser.id = "images.structure".into();
        let result = reconcile_image_metadata(&[a.clone(), b.clone()]).unwrap();
        assert_eq!(result.claims[0].agreement, AgreementLevel::SingleParser);
        b.provenance.parser.library = Some(LibraryDescriptor {
            name: "independent".into(),
            version: "reviewed".into(),
        });
        assert_eq!(
            reconcile_image_metadata(&[a.clone(), b]).unwrap().claims[0].agreement,
            AgreementLevel::Corroborated
        );
        assert_eq!(
            reconcile_image_metadata(&[a.clone(), a.clone()]),
            Err(ImageNormalizationError::DuplicateObservationId)
        );
        let mut other = a.clone();
        other.id = ObservationId::new(3);
        other.provenance.source = EvidencePath::root(RelativePath::parse("other.png").unwrap());
        assert_eq!(
            reconcile_image_metadata(&[a, other]),
            Err(ImageNormalizationError::MixedSources)
        );
        let hostile = row(4, "image.ico.entry!bad", "width", RawValue::Unsigned(3));
        assert!(
            reconcile_image_metadata(&[hostile])
                .unwrap()
                .claims
                .is_empty()
        );
    }

    #[test]
    fn color_channels_time_precision_and_analysis_limits_are_explicit() {
        let mut scoped = vec![
            row(10, "image.tiff.ifd0", "width", RawValue::Unsigned(3)),
            row(11, "image.tiff.ifd0", "height", RawValue::Unsigned(2)),
            row(12, "image.tiff.ifd0", "orientation", RawValue::Unsigned(6)),
            row(13, "image.tiff.ifd2", "orientation", RawValue::Unsigned(3)),
            row(
                14,
                "image.tiff.ifd0",
                "photometric_interpretation",
                RawValue::Unsigned(2),
            ),
            row(15, "image.tiff.ifd2.icc", "color_space", string("GRAY")),
        ];
        let result = reconcile_image_metadata(&scoped).unwrap();
        assert_eq!(result.display_dimensions, Some((2, 3)));
        assert!(result.conflicts.is_empty());
        assert!(
            !result
                .issues
                .iter()
                .any(|issue| issue.kind == ImageIssueKind::ColorSpaceMismatch)
        );
        scoped[2].interpreted_value = Some(RawValue::Unsigned(9));
        assert_eq!(
            reconcile_image_metadata(&scoped)
                .unwrap()
                .display_dimensions,
            None
        );
        let rows = vec![
            row(1, "image.png", "color_type", RawValue::Unsigned(0)),
            row(2, "image.png.iCCP", "color_space", string("RGB ")),
        ];
        assert!(
            reconcile_image_metadata(&rows)
                .unwrap()
                .issues
                .iter()
                .any(|issue| issue.kind == ImageIssueKind::ColorSpaceMismatch)
        );
        let times = vec![
            row(
                3,
                "image.png.xmp.node0",
                "{http://ns.adobe.com/exif/1.0/}DateTimeOriginal",
                string("1970-01-01T00:00:00Z"),
            ),
            row(
                4,
                "image.png.xmp.node1",
                "{http://ns.adobe.com/exif/1.0/}DateTimeOriginal",
                string("1970-01-01T00:00:00.125Z"),
            ),
        ];
        assert!(
            reconcile_image_metadata(&times)
                .unwrap()
                .conflicts
                .is_empty()
        );
        let differing = vec![
            row(5, "image.png", "width", RawValue::Unsigned(3)),
            row(6, "image.png", "width", RawValue::Unsigned(4)),
        ];
        assert_eq!(
            reconcile_image_metadata_with_limits(
                &differing,
                ImageAnalysisLimits {
                    max_alternatives_per_key: 1,
                    ..ImageAnalysisLimits::default()
                }
            ),
            Err(ImageNormalizationError::AnalysisLimitExceeded)
        );
        assert_eq!(
            reconcile_image_metadata_with_limits(
                &differing,
                ImageAnalysisLimits {
                    max_claim_links: 1,
                    ..ImageAnalysisLimits::default()
                }
            ),
            Err(ImageNormalizationError::AnalysisLimitExceeded)
        );
    }
}
