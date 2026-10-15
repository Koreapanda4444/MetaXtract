use crate::{Result, invalid};
use serde::{Serialize, de::DeserializeOwned};
use std::io::{self, Write};

pub const MAX_RECORD_BYTES: usize = 256 * 1024 * 1024;
struct Writer {
    data: Vec<u8>,
    limit: usize,
}
impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.data.len()) {
            return Err(invalid("case value exceeds its byte budget"));
        }
        self.data
            .try_reserve(bytes.len())
            .map_err(io::Error::other)?;
        self.data.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(crate) fn encode(value: &impl Serialize, max: usize) -> Result<Vec<u8>> {
    let mut writer = Writer {
        data: Vec::new(),
        limit: max,
    };
    serde_json::to_writer(&mut writer, value).map_err(io::Error::other)?;
    Ok(writer.data)
}
pub(crate) fn decode<T: DeserializeOwned>(bytes: &[u8], max: usize) -> Result<T> {
    if bytes.len() > max {
        return Err(invalid("stored case value exceeds its byte budget"));
    }
    serde_json::from_slice(bytes).map_err(io::Error::other)
}
