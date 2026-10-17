use serde_json::Value;
use std::io::{self, Write};
pub fn write(value: &Value, pretty: bool) -> io::Result<()> {
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    let result = if pretty {
        serde_json::to_writer_pretty(&mut stdout, value)
    } else {
        serde_json::to_writer(&mut stdout, value)
    };
    result.map_err(|e| io::Error::new(e.io_error_kind().unwrap_or(io::ErrorKind::Other), e))?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}
pub fn error_code(e: &io::Error) -> i32 {
    match e.kind() {
        io::ErrorKind::InvalidInput => 2,
        io::ErrorKind::InvalidData => 4,
        io::ErrorKind::Interrupted => 130,
        _ => 3,
    }
}
pub fn error_kind(e: &io::Error) -> &'static str {
    match e.kind() {
        io::ErrorKind::InvalidInput => "invalid_input",
        io::ErrorKind::InvalidData => "invalid_data",
        io::ErrorKind::NotFound => "not_found",
        io::ErrorKind::PermissionDenied => "permission_denied",
        io::ErrorKind::AlreadyExists => "already_exists",
        io::ErrorKind::Interrupted => "interrupted",
        _ => "io_error",
    }
}
pub fn result_code(v: &Value) -> i32 {
    if v.get("verified").and_then(Value::as_bool) == Some(false) {
        return 4;
    }
    let s = if v.get("records").is_some() {
        v.get("summary").and_then(|x| x.get("summary"))
    } else {
        v.get("summary")
    };
    if let Some(s) = s {
        if s.get("stopped").and_then(Value::as_str) == Some("Cancelled") {
            return 130;
        }
        if s.get("stopped").is_some_and(|v| !v.is_null()) {
            return 6;
        }
        if s.get("failed").and_then(Value::as_u64).unwrap_or(0) > 0
            || s.get("unstable").and_then(Value::as_u64).unwrap_or(0) > 0
        {
            return 5;
        }
    }
    0
}
