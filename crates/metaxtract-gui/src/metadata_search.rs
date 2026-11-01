use metaxtract_app::schema::{NormalizedValue, RawValue};
pub fn contains(text: &str, needle: &str) -> bool {
    text.to_lowercase().contains(needle)
}
pub fn raw(value: &RawValue, needle: &str) -> bool {
    match value {
        RawValue::Text(v) => contains(&v.text, needle),
        RawValue::Sequence(v) => v.iter().any(|v| raw(v, needle)),
        RawValue::Structure(v) => v
            .iter()
            .any(|v| contains(&v.name, needle) || raw(&v.value, needle)),
        RawValue::Bytes(_) => false,
        value => contains(&crate::formatting::raw(value, None), needle),
    }
}
pub fn normalized(value: &NormalizedValue, needle: &str) -> bool {
    match value {
        NormalizedValue::Text(v) => contains(v, needle),
        NormalizedValue::Sequence(v) => v.iter().any(|v| normalized(v, needle)),
        v => contains(&crate::formatting::normalized(v), needle),
    }
}
