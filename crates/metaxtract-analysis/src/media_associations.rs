use crate::{
    AnalysisLimits, associations::charge, encoded_size, invalid, relationships::*, schema::*,
};
use std::{collections::BTreeMap, io};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TakeoutMetadata {
    pub title: String,
    pub reported_capture_seconds: Option<i64>,
    pub reported_location: Option<GeoPoint>,
    pub location_source: Option<String>,
    pub zero_location_ambiguous: bool,
    pub packet_observation: ObservationId,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum MediaAssociationBasis {
    LivePhotoFilenameCandidate,
    LivePhotoDeclaredIdentifier {
        identifier: String,
        still_observation: ObservationId,
        movie_observation: ObservationId,
    },
    TakeoutDeclaredTitle {
        metadata: TakeoutMetadata,
    },
}

pub fn takeout_metadata(record: &ExtractionRecord) -> io::Result<Option<TakeoutMetadata>> {
    let Some(packet) = record.observations.iter().find(|o| {
        o.provenance.parser.id == "metadata.export" && o.tag.name.as_deref() == Some("json_packet")
    }) else {
        return Ok(None);
    };
    let RawValue::Bytes(data) = &packet.raw_value else {
        return Err(invalid("exported metadata packet is not byte-preserving"));
    };
    if data.len() > 2 * 1024 * 1024 {
        return Err(invalid("exported metadata packet exceeds 2 MiB"));
    }
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    let value: serde_json::Value = serde_json::from_slice(data)
        .map_err(|_| invalid("exported metadata is not valid bounded JSON"))?;
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let Some(title) = object.get("title").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    if !object.contains_key("photoTakenTime")
        && !object.contains_key("geoData")
        && !object.contains_key("geoDataExif")
    {
        return Ok(None);
    }
    if title.is_empty()
        || title.len() > 4096
        || title.contains(['/', '\\'])
        || title.chars().any(char::is_control)
        || matches!(title, "." | "..")
    {
        return Err(invalid("exported title must be a single bounded filename"));
    }
    let reported_capture_seconds = object
        .get("photoTakenTime")
        .map(|time| {
            let timestamp = time
                .get("timestamp")
                .ok_or_else(|| invalid("exported capture timestamp is missing"))?;
            let seconds = timestamp
                .as_str()
                .and_then(|t| t.parse::<i64>().ok())
                .or_else(|| timestamp.as_i64())
                .ok_or_else(|| invalid("exported capture timestamp is invalid"))?;
            if !(-62_135_596_800..=253_402_300_799).contains(&seconds) {
                return Err(invalid(
                    "exported capture timestamp is outside supported civil dates",
                ));
            }
            Ok(seconds)
        })
        .transpose()?;
    let location = object
        .get("geoData")
        .map(|v| ("geoData", v))
        .or_else(|| object.get("geoDataExif").map(|v| ("geoDataExif", v)));
    let (reported_location, location_source) = if let Some((name, location)) = location {
        let point = GeoPoint {
            latitude: location
                .get("latitude")
                .and_then(serde_json::Value::as_f64)
                .ok_or_else(|| invalid("exported latitude is missing"))?,
            longitude: location
                .get("longitude")
                .and_then(serde_json::Value::as_f64)
                .ok_or_else(|| invalid("exported longitude is missing"))?,
            altitude_meters: location.get("altitude").and_then(serde_json::Value::as_f64),
            horizontal_accuracy_meters: None,
        };
        if !point.is_valid() {
            return Err(invalid(
                "exported coordinates are outside geographic bounds",
            ));
        }
        (Some(point), Some(name.to_owned()))
    } else {
        (None, None)
    };
    Ok(Some(TakeoutMetadata {
        title: title.into(),
        reported_capture_seconds,
        reported_location,
        location_source,
        zero_location_ambiguous: reported_location
            .is_some_and(|p| p.latitude == 0.0 && p.longitude == 0.0),
        packet_observation: packet.id,
    }))
}
fn content_ids(record: &ExtractionRecord) -> BTreeMap<String, ObservationId> {
    record
        .observations
        .iter()
        .filter_map(|o| {
            let apple = matches!(
                o.provenance.parser.id.as_str(),
                "images.metadata" | "images.heif"
            ) && o.tag.name.as_deref() == Some("apple_content_identifier");
            let movie = o.provenance.parser.id == "media.inspect"
                && o.tag.name.as_deref()
                    == Some("metadata.com_2eapple_2equicktime_2econtent_2eidentifier");
            if !apple && !movie {
                return None;
            }
            let RawValue::Text(t) = o.interpreted_value.as_ref().unwrap_or(&o.raw_value) else {
                return None;
            };
            if !t.lossless || t.text.is_empty() || t.text.len() > 4096 {
                return None;
            }
            let value = t.text.trim_end_matches('\0');
            let uuid = value.len() == 36
                && value.bytes().enumerate().all(|(i, c)| {
                    if [8, 13, 18, 23].contains(&i) {
                        c == b'-'
                    } else {
                        c.is_ascii_hexdigit()
                    }
                });
            Some((
                if uuid {
                    value.to_ascii_lowercase()
                } else {
                    value.to_owned()
                },
                o.id,
            ))
        })
        .collect()
}
fn media_kind(record: &ExtractionRecord) -> Option<bool> {
    let path = record.source.path.root_path().as_str().to_ascii_lowercase();
    if [".jpg", ".jpeg", ".heic", ".heif"]
        .iter()
        .any(|ext| path.ends_with(ext))
    {
        Some(true)
    } else if [".mov", ".mp4"].iter().any(|ext| path.ends_with(ext)) {
        Some(false)
    } else {
        None
    }
}
pub(crate) fn append_media(
    records: &[&ExtractionRecord],
    graph: &mut RelationshipGraph,
    limits: AnalysisLimits,
    bytes: &mut usize,
) -> io::Result<()> {
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    let mut stills = vec![];
    let mut movies: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut movie_ids: BTreeMap<String, Vec<(usize, ObservationId)>> = BTreeMap::new();
    let mut identities = BTreeMap::new();
    let mut exports = vec![];
    for (index, record) in records.iter().enumerate() {
        if record.source.path.depth() != 0 || record.source.kind != SourceKind::File {
            continue;
        }
        let path = record.source.path.root_path().as_str();
        charge(bytes, path.len().saturating_add(128), limits)?;
        if record
            .observations
            .iter()
            .any(|o| o.provenance.parser.id == "metadata.export")
        {
            // Invalid packets are diagnosed during scan; they never become links.
            if let Ok(Some(metadata)) = takeout_metadata(record) {
                exports.push((index, metadata));
            }
            continue;
        }
        names.insert(path.to_owned(), index);
        let Some(still) = media_kind(record) else {
            continue;
        };
        let ids = content_ids(record);
        for id in ids.keys() {
            charge(bytes, id.len().saturating_add(128), limits)?;
        }
        identities.insert(index, ids.clone());
        if still {
            stills.push(index);
        } else {
            let stem = path.rsplit_once('.').map(|p| p.0).unwrap_or(path);
            movies.entry(stem.to_owned()).or_default().push(index);
            for (id, observation) in ids {
                movie_ids.entry(id).or_default().push((index, observation));
            }
        }
    }
    let mut comparisons = 0usize;
    let mut retain =
        |first: usize, second: usize, evidence: MediaAssociationBasis| -> io::Result<()> {
            comparisons = comparisons.saturating_add(1);
            if comparisons > limits.max_comparisons || graph.edges.len() >= limits.max_results {
                return Err(invalid("media association result budget exceeded"));
            }
            let basis = RelationshipBasis::RelatedMedia { evidence };
            charge(bytes, encoded_size(&basis, limits.max_bytes)?, limits)?;
            graph.edges.push(Relationship {
                first,
                second,
                basis,
            });
            Ok(())
        };
    for still in stills {
        let path = records[still].source.path.root_path().as_str();
        let ids = &identities[&still];
        let mut exact = std::collections::BTreeSet::new();
        for (id, still_observation) in ids {
            if let Some(targets) = movie_ids.get(id) {
                for (movie, movie_observation) in targets {
                    retain(
                        still,
                        *movie,
                        MediaAssociationBasis::LivePhotoDeclaredIdentifier {
                            identifier: id.clone(),
                            still_observation: *still_observation,
                            movie_observation: *movie_observation,
                        },
                    )?;
                    exact.insert(*movie);
                }
            }
        }
        let stem = path.rsplit_once('.').map(|p| p.0).unwrap_or(path);
        if let Some(candidates) = movies.get(stem) {
            for movie in candidates {
                if exact.contains(movie) || (!ids.is_empty() && !identities[movie].is_empty()) {
                    continue;
                }
                retain(
                    still,
                    *movie,
                    MediaAssociationBasis::LivePhotoFilenameCandidate,
                )?;
            }
        }
    }
    for (index, metadata) in exports {
        let path = records[index].source.path.root_path().as_str();
        let parent = path.rsplit_once('/').map(|p| p.0).unwrap_or("");
        let target = if parent.is_empty() {
            metadata.title.clone()
        } else {
            format!("{parent}/{}", metadata.title)
        };
        if let Some(source) = names.get(&target) {
            retain(
                *source,
                index,
                MediaAssociationBasis::TakeoutDeclaredTitle { metadata },
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(path: &str) -> ExtractionRecord {
        ExtractionRecord::new(SourceDescriptor {
            path: EvidencePath::root(RelativePath::parse(path).unwrap()),
            kind: SourceKind::File,
            identity: FileIdentity {
                size_bytes: 1024,
                modified_ns: None,
                created_ns: None,
                stable_id: None,
            },
            declared_media_type: None,
            detected_media_type: None,
        })
    }
    fn field(record: &mut ExtractionRecord, parser: &str, key: &str, value: RawValue) {
        let id = ObservationId::new(record.observations.len() as u64);
        record.observations.push(MetadataObservation {
            id,
            tag: TagIdentity {
                namespace: "test".into(),
                code: TagCode::Text(key.into()),
                name: Some(key.into()),
            },
            raw_value: value,
            interpreted_value: None,
            unit: None,
            provenance: Provenance {
                parser: ParserDescriptor {
                    id: parser.into(),
                    name: parser.into(),
                    version: "test".into(),
                    library: None,
                },
                source: record.source.path.clone(),
                stream_id: None,
                source_range: None,
                method: ExtractionMethod::DecodedPayload,
                sequence: id.get(),
            },
        });
    }
    fn string(value: &str) -> RawValue {
        RawValue::Text(TextValue {
            text: value.into(),
            original_encoding: None,
            language: None,
            encoded_bytes: None,
            lossless: true,
        })
    }
    #[test]
    fn live_photo_ids_override_names_and_conflicting_ids_block_name_fallback() {
        let mut still = record("a.jpg");
        let mut movie = record("a.mov");
        field(
            &mut still,
            "images.metadata",
            "apple_content_identifier",
            string("ABCD0000-0000-0000-0000-000000000001"),
        );
        field(
            &mut movie,
            "media.inspect",
            "metadata.com_2eapple_2equicktime_2econtent_2eidentifier",
            string("abcd0000-0000-0000-0000-000000000001"),
        );
        let graph = build(&[still.clone(), movie.clone()], Default::default()).unwrap();
        assert_eq!(graph.edges.len(), 1);
        assert!(matches!(
            graph.edges[0].basis,
            RelationshipBasis::RelatedMedia {
                evidence: MediaAssociationBasis::LivePhotoDeclaredIdentifier { .. }
            }
        ));
        movie.observations[0].raw_value = string("different-id");
        assert!(
            build(&[still, movie], Default::default())
                .unwrap()
                .edges
                .is_empty()
        );
    }
    #[test]
    fn filename_pairs_are_only_candidates_and_do_not_cross_folders() {
        let graph = build(
            &[record("a.jpg"), record("a.mov"), record("other/a.mov")],
            Default::default(),
        )
        .unwrap();
        assert_eq!(graph.edges.len(), 1);
        assert!(matches!(
            graph.edges[0].basis,
            RelationshipBasis::RelatedMedia {
                evidence: MediaAssociationBasis::LivePhotoFilenameCandidate
            }
        ));
    }
    #[test]
    fn takeout_uses_declared_title_in_its_folder_without_mutating_sources() {
        let mut export = record("photos/truncated.supplemental-metadata.json");
        field(&mut export, "metadata.export", "json_packet", RawValue::Bytes(br#"{"title":"photo.jpg","photoTakenTime":{"timestamp":"1600000000"},"creationTime":{"timestamp":"1700000000"},"geoData":{"latitude":0,"longitude":0}}"#.to_vec()));
        let records = [
            record("photos/photo.jpg"),
            record("elsewhere/photo.jpg"),
            export,
        ];
        let before = serde_json::to_vec(&records).unwrap();
        let graph = build(&records, Default::default()).unwrap();
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(
            graph.files[graph.edges[0].first].root_path().as_str(),
            "photos/photo.jpg"
        );
        let RelationshipBasis::RelatedMedia {
            evidence: MediaAssociationBasis::TakeoutDeclaredTitle { metadata },
        } = &graph.edges[0].basis
        else {
            panic!("missing export association");
        };
        assert_eq!(metadata.reported_capture_seconds, Some(1600000000));
        assert!(metadata.zero_location_ambiguous);
        assert_eq!(serde_json::to_vec(&records).unwrap(), before);
    }
    #[test]
    fn malformed_exports_and_album_only_objects_never_become_links() {
        let mut export = record("a.jpg.json");
        for data in [
            r#"{"title":"../a.jpg","photoTakenTime":{"timestamp":"1"}}"#,
            r#"{"title":"a.jpg","photoTakenTime":{"timestamp":"bad"}}"#,
            r#"{"title":"a.jpg","geoData":{"latitude":1000,"longitude":0}}"#,
            "{broken",
        ] {
            export.observations.clear();
            field(
                &mut export,
                "metadata.export",
                "json_packet",
                RawValue::Bytes(data.as_bytes().to_vec()),
            );
            assert!(takeout_metadata(&export).is_err());
            assert!(
                build(&[record("a.jpg"), export.clone()], Default::default())
                    .unwrap()
                    .edges
                    .is_empty()
            );
        }
        export.observations.clear();
        field(
            &mut export,
            "metadata.export",
            "json_packet",
            RawValue::Bytes(br#"{"title":"a.jpg"}"#.to_vec()),
        );
        assert!(takeout_metadata(&export).unwrap().is_none());
    }
}
