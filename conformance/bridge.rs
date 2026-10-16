use std::{
    error::Error,
    fs::{File, OpenOptions},
    io::{self, BufRead},
};

use metaxtract_native::{
    BoundedInput, InputLimits, NativeError, NativeErrorCode, ParserLimits, build_info, check_input,
    detect_content, extract_parser, parser_capabilities, read_input, run_parser,
};
use metaxtract_schema::{
    ByteRange, Confidence, EvidenceKind, EvidencePath, EvidenceSegment, GeoPoint, RawValue,
    RelativePath,
};

fn decode(value: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    if !value.len().is_multiple_of(2) {
        return Err("invalid hex length".into());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| Ok(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?))
        .collect()
}

fn encode(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn text(value: &str) -> Result<String, Box<dyn Error>> {
    Ok(String::from_utf8(decode(value)?)?)
}

fn rendered(value: &RawValue) -> String {
    match value {
        RawValue::Unsigned(value) => value.to_string(),
        RawValue::Signed(value) => value.to_string(),
        RawValue::Float(value) => value.to_string(),
        RawValue::Text(value) => encode(value.text.as_bytes()),
        RawValue::Bytes(value) => encode(value),
        RawValue::Rational(value) => format!("{}/{}", value.numerator, value.denominator),
        RawValue::Sequence(values) => values.iter().map(rendered).collect::<Vec<_>>().join(","),
        _ => "unsupported".to_owned(),
    }
}

fn failure(error: NativeError) {
    println!("ERR\t{:?}\t{}", error.code, error.recoverable);
}

fn fuzz(bytes: Vec<u8>, parser: &str, mode: &str) -> Result<(), Box<dyn Error>> {
    let mut limits = ParserLimits {
        max_source_bytes: 65536,
        max_read_bytes: 4 * 1024 * 1024,
        max_allocation_bytes: 256 * 1024 * 1024,
        max_decoded_bytes: 1024 * 1024,
        max_container_depth: 8,
        max_entries: 256,
        max_observations: 256,
        max_wall_time_ms: 250,
        max_member_bytes: 1024 * 1024,
        max_expansion_ratio: 1000,
    };
    match mode {
        "normal" | "short" => {}
        "read" => limits.max_read_bytes = 1,
        "allocation" => limits.max_allocation_bytes = 1,
        "decoded" => limits.max_decoded_bytes = 0,
        "depth" => limits.max_container_depth = 0,
        "entries" => limits.max_entries = 0,
        "observations" => limits.max_observations = 0,
        "member" => limits.max_member_bytes = 0,
        "ratio" => limits.max_expansion_ratio = 1,
        "allocation-near" => limits.max_allocation_bytes = 65 * 1024,
        _ => return Err("unknown fuzz budget mode".into()),
    }
    let input = BoundedInput::from_bytes(
        bytes.clone(),
        InputLimits {
            max_single_read: if mode == "short" { 1 } else { 1024 * 1024 },
            ..limits.input_limits()
        },
    )?;
    let unexpected = |error: &NativeError| {
        matches!(
            error.code,
            NativeErrorCode::Internal
                | NativeErrorCode::Unknown(_)
                | NativeErrorCode::RevisionMismatch
                | NativeErrorCode::AllocationFailure
        )
    };
    match detect_content(&input, limits) {
        Ok(values)
            if values
                .iter()
                .any(|v| v.source_range.end() > bytes.len() as u64) =>
        {
            return Err("detector fabricated an out-of-source range".into());
        }
        Err(error) if unexpected(&error) => return Err(error.into()),
        _ => {}
    }
    let mut tested = 0;
    for capability in parser_capabilities()? {
        if capability.id.starts_with("conformance.") || (parser != "*" && capability.id != parser) {
            continue;
        }
        tested += 1;
        let result = extract_parser(&input, &capability.id, limits)
            .map_err(|error| format!("{}: {error}", capability.id))?;
        if let Some(error) = &result.error {
            if unexpected(error) {
                return Err(format!("{}: {error}", capability.id).into());
            }
        }
        let usage = result.run;
        if usage.read_bytes > limits.max_read_bytes
            || usage.allocation_bytes > limits.max_allocation_bytes
            || usage.decoded_bytes > limits.max_decoded_bytes
            || usage.entries > limits.max_entries
            || usage.observations > limits.max_observations
            || result.observations.len() > usage.observations as usize
        {
            return Err(format!("{} exceeded a parser budget", capability.id).into());
        }
        for observation in result.observations {
            if let Some(range) = observation.source_range {
                if range.end() > bytes.len() as u64
                    || bytes[range.offset() as usize..range.end() as usize] != observation.encoded
                {
                    return Err(format!("{} fabricated source bytes", capability.id).into());
                }
            }
            if matches!(
                observation.method,
                metaxtract_schema::ExtractionMethod::DecodedPayload
                    | metaxtract_schema::ExtractionMethod::NativeLibrary
            ) && (observation.source_range.is_some() || observation.stream_id.is_none())
            {
                return Err(format!("{} lost decoded provenance", capability.id).into());
            }
        }
    }
    if tested == 0 {
        return Err("fuzz requested an unregistered production parser".into());
    }
    Ok(())
}

fn schema_check() -> Result<(), Box<dyn Error>> {
    if ByteRange::new(u64::MAX, 1).is_some()
        || ByteRange::new(u64::MAX, 0)
            .ok_or("missing empty range")?
            .end()
            != u64::MAX
        || !ByteRange::new(10, 20)
            .ok_or("missing range")?
            .contains(ByteRange::new(12, 4).ok_or("missing subrange")?)
        || Confidence::new(1001, vec![]).is_some()
        || Confidence::new(1000, vec![]).is_none()
    {
        return Err("range or confidence invariant failed".into());
    }
    let point = GeoPoint {
        latitude: 90.0,
        longitude: -180.0,
        altitude_meters: None,
        horizontal_accuracy_meters: None,
    };
    if !point.is_valid()
        || (GeoPoint {
            latitude: f64::NAN,
            ..point
        })
        .is_valid()
    {
        return Err("coordinate invariant failed".into());
    }
    let root = RelativePath::parse("case.zip")?;
    let child = RelativePath::parse("same.txt")?;
    let first = EvidencePath::root(root.clone()).with_segment(EvidenceSegment::new(
        EvidenceKind::ArchiveMember,
        child.clone(),
        0,
    ));
    let second = EvidencePath::root(root.clone()).with_segment(EvidenceSegment::new(
        EvidenceKind::ArchiveMember,
        child.clone(),
        1,
    ));
    let attachment = EvidencePath::root(root).with_segment(EvidenceSegment::new(
        EvidenceKind::Attachment,
        child,
        0,
    ));
    let literal = EvidencePath::root(RelativePath::parse(first.to_string())?);
    if first.to_string() == second.to_string()
        || first.to_string() == attachment.to_string()
        || first.to_string() == literal.to_string()
    {
        return Err("virtual evidence path collision".into());
    }
    println!("SCHEMA\tOK");
    Ok(())
}

fn dispatch(fields: &[&str]) -> Result<(), Box<dyn Error>> {
    match fields {
        ["FUZZ", label, value, parser, mode] => {
            eprintln!("fuzz checkpoint: {label} ({parser}/{mode})");
            fuzz(decode(value)?, parser, mode)
                .map_err(|error| format!("fuzz case {label} ({parser}/{mode}): {error}"))?;
            println!("FUZZ\tOK");
        }
        ["INFO"] => {
            let info = build_info()?;
            println!(
                "INFO\t{}\t{}\t{}",
                info.api_revision,
                info.schema_revision,
                parser_capabilities()?.len()
            );
        }
        ["SCHEMA"] => schema_check()?,
        [
            "EXTRACT",
            value,
            parser,
            selected,
            read_max,
            observation_max,
            options @ ..,
        ] => {
            let bytes = decode(value)?;
            let input = BoundedInput::from_bytes(bytes.clone(), InputLimits::default())?;
            let result = extract_parser(
                &input,
                parser,
                ParserLimits {
                    max_read_bytes: read_max.parse()?,
                    max_observations: observation_max.parse()?,
                    max_allocation_bytes: options
                        .first()
                        .map(|v| v.parse())
                        .transpose()?
                        .unwrap_or(ParserLimits::default().max_allocation_bytes),
                    max_decoded_bytes: options
                        .get(1)
                        .map(|v| v.parse())
                        .transpose()?
                        .unwrap_or(ParserLimits::default().max_decoded_bytes),
                    max_container_depth: options
                        .get(2)
                        .map(|v| v.parse())
                        .transpose()?
                        .unwrap_or(ParserLimits::default().max_container_depth),
                    max_entries: options
                        .get(3)
                        .map(|v| v.parse())
                        .transpose()?
                        .unwrap_or(ParserLimits::default().max_entries),
                    max_member_bytes: options
                        .get(4)
                        .map(|v| v.parse())
                        .transpose()?
                        .unwrap_or(ParserLimits::default().max_member_bytes),
                    max_expansion_ratio: options
                        .get(5)
                        .map(|v| v.parse())
                        .transpose()?
                        .unwrap_or(ParserLimits::default().max_expansion_ratio),
                    ..ParserLimits::default()
                },
            )?;
            for observation in &result.observations {
                if let Some(range) = observation.source_range {
                    if range.end() > bytes.len() as u64
                        || bytes[range.offset() as usize..range.end() as usize]
                            != observation.encoded
                    {
                        return Err("raw observation does not match its source byte range".into());
                    }
                }
                if matches!(
                    observation.method,
                    metaxtract_schema::ExtractionMethod::DecodedPayload
                        | metaxtract_schema::ExtractionMethod::NativeLibrary
                ) && (observation.source_range.is_some() || observation.stream_id.is_none())
                {
                    return Err(
                        "decoded field has fabricated source coordinates or no packet provenance"
                            .into(),
                    );
                }
            }
            let fields = selected
                .split(',')
                .filter(|key| !key.is_empty())
                .map(|key| {
                    let values = result
                        .observations
                        .iter()
                        .filter(|value| value.key == key)
                        .map(|value| rendered(&value.value))
                        .collect::<Vec<_>>();
                    format!("{key}={}", values.join("|"))
                })
                .collect::<Vec<_>>()
                .join(";");
            let status = result
                .error
                .as_ref()
                .map(|error| format!("{:?}", error.code))
                .unwrap_or_else(|| "OK".into());
            println!("EXTRACT\t{status}\t{fields}");
        }
        ["DETECT", value, maximum, read_max, observation_max] => {
            let input = BoundedInput::from_bytes(decode(value)?, InputLimits::default())?;
            let limits = ParserLimits {
                max_source_bytes: maximum.parse()?,
                max_read_bytes: read_max.parse()?,
                max_observations: observation_max.parse()?,
                ..ParserLimits::default()
            };
            match detect_content(&input, limits) {
                Ok(values) => {
                    let values = values
                        .iter()
                        .map(|value| {
                            format!(
                                "{}:{:?}:{}",
                                value.format,
                                value.state,
                                value.source_range.offset()
                            )
                        })
                        .collect::<Vec<_>>();
                    println!("DETECT\t{}", values.join(","));
                }
                Err(error) => failure(error),
            }
        }
        ["PATH", value] => match RelativePath::parse(text(value)?) {
            Ok(path) => println!("PATH\tOK\t{}", encode(path.as_str().as_bytes())),
            Err(_) => println!("PATH\tERR"),
        },
        [
            kind @ ("MEM" | "FILE" | "CHECK"),
            value,
            source_max,
            single_max,
            total_max,
            requests,
        ] => {
            let limits = InputLimits {
                max_source_bytes: source_max.parse()?,
                max_single_read: single_max.parse()?,
                max_total_read: total_max.parse()?,
            };
            let input = if *kind == "MEM" {
                BoundedInput::from_bytes(decode(value)?, limits)
            } else {
                BoundedInput::from_file(File::open(text(value)?)?, limits)
            };
            match input {
                Err(error) => failure(error),
                Ok(input) => {
                    for request in requests.split(',') {
                        let (offset, length) = request.split_once(':').ok_or("invalid request")?;
                        if *kind == "CHECK" {
                            let result = check_input(&input, length.parse()?);
                            match result {
                                Ok(result) => {
                                    println!("CHECK\t{}\t{}", result.source_size, result.bytes_read)
                                }
                                Err(error) => failure(error),
                            }
                        } else {
                            match read_input(&input, offset.parse()?, length.parse()?) {
                                Ok(result) => println!(
                                    "READ\t{}\t{}",
                                    result.source_size,
                                    encode(&result.bytes)
                                ),
                                Err(error) => failure(error),
                            }
                        }
                    }
                }
            }
        }
        ["RUN", value, maximum, wall_time] => {
            let input = BoundedInput::from_file(File::open(text(value)?)?, InputLimits::default())?;
            let limits = ParserLimits {
                max_source_bytes: maximum.parse()?,
                max_wall_time_ms: wall_time.parse()?,
                ..ParserLimits::default()
            };
            match run_parser(&input, "not-registered", limits) {
                Ok(_) => println!("RUN\tOK"),
                Err(error) => failure(error),
            }
        }
        [
            "PARSE",
            value,
            read_max,
            allocation_max,
            decoded_max,
            wall_time,
            repetitions,
        ] => {
            let input = BoundedInput::from_bytes(
                decode(value)?,
                InputLimits {
                    max_source_bytes: 4,
                    max_single_read: 4,
                    max_total_read: 4,
                },
            )?;
            let limits = ParserLimits {
                max_source_bytes: 4,
                max_read_bytes: read_max.parse()?,
                max_allocation_bytes: allocation_max.parse()?,
                max_decoded_bytes: decoded_max.parse()?,
                max_container_depth: 2,
                max_entries: 2,
                max_observations: 2,
                max_wall_time_ms: wall_time.parse()?,
                ..ParserLimits::default()
            };
            for _ in 0..repetitions.parse::<usize>()? {
                match run_parser(&input, "conformance.resource_limits", limits) {
                    Ok(result) => {
                        println!("PARSE\t{}\t{}", result.read_bytes, result.allocation_bytes)
                    }
                    Err(error) => failure(error),
                }
            }
        }
        [kind @ ("SHRINK" | "SHRINK_DETECT"), value] => {
            let path = text(value)?;
            let input = BoundedInput::from_file(File::open(&path)?, InputLimits::default())?;
            OpenOptions::new().write(true).open(path)?.set_len(0)?;
            let result = if *kind == "SHRINK" {
                read_input(&input, 0, 1).map(|_| ())
            } else {
                detect_content(&input, ParserLimits::default()).map(|_| ())
            };
            match result {
                Ok(_) => println!("SHRINK\tUNEXPECTED_SUCCESS"),
                Err(error) => failure(error),
            }
        }
        _ => return Err(format!("invalid conformance command: {fields:?}").into()),
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    for line in io::stdin().lock().lines() {
        let line = line?;
        dispatch(&line.split('\t').collect::<Vec<_>>())?;
    }
    Ok(())
}
