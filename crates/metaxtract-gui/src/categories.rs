use metaxtract_app::schema::{ExtractionRecord, MetadataObservation};
use std::collections::BTreeMap;

pub fn category(observation: &MetadataObservation) -> &'static str {
    let namespace = observation.tag.namespace.to_ascii_lowercase();
    let name = observation
        .tag
        .name
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    if namespace.contains("gps")
        || name.contains("latitude")
        || name.contains("longitude")
        || name.starts_with("gps")
    {
        "GPS"
    } else if namespace.contains("xmp") {
        "XMP"
    } else if namespace.contains("iptc") {
        "IPTC"
    } else if namespace.contains("exif") || namespace.contains("tiff") {
        "EXIF"
    } else if namespace.starts_with("audio")
        || namespace.contains("id3")
        || namespace.contains("vorbis")
    {
        "Audio"
    } else if namespace.starts_with("video") || namespace.starts_with("media") {
        "Video and media"
    } else if namespace.starts_with("document")
        || namespace.starts_with("pdf")
        || namespace.contains("office")
        || namespace.contains("ole")
    {
        "Document"
    } else if namespace.starts_with("archive") || namespace.starts_with("zip") {
        "Archive"
    } else if namespace.starts_with("image") {
        "Image"
    } else if namespace.starts_with("font") {
        "Font"
    } else if namespace.starts_with("binary") || namespace.starts_with("executable") {
        "Executable"
    } else {
        "General"
    }
}
pub fn groups(record: &ExtractionRecord) -> Vec<(String, Vec<usize>)> {
    let mut groups = BTreeMap::<String, Vec<usize>>::new();
    for (index, observation) in record.observations.iter().enumerate() {
        groups
            .entry(category(observation).into())
            .or_default()
            .push(index);
    }
    groups.into_iter().collect()
}
