use crate::{invalid, schema::CanonicalKey};
pub use metaxtract_storage::{
    FacetField, FacetPage, FacetValue, SearchExpression, SearchField, SearchOperator,
    SearchPredicate, SearchQuery,
};
use std::io;

/// Whitespace AND, quoted values, and a leading '-' for negation. OR groups use the typed AST.
pub fn parse(scan_id: &str, input: &str) -> io::Result<SearchQuery> {
    if input.len() > 16 * 1024 {
        return Err(invalid("query exceeds 16 KiB"));
    }
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for c in input.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }
        if c == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                current.push(c);
            }
            continue;
        }
        if matches!(c, '\'' | '"') {
            quote = Some(c);
        } else if c.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if quote.is_some() || escaped {
        return Err(invalid("unterminated quote or escape in search"));
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    if tokens.len() > 64 {
        return Err(invalid("query exceeds 64 terms"));
    }
    let mut expressions = Vec::new();
    for token in tokens {
        let (negate, token) = if let Some(t) = token.strip_prefix('-') {
            (true, t)
        } else {
            (false, token.as_str())
        };
        if token.is_empty() {
            return Err(invalid("empty negated query term"));
        }
        let mut parsed = None;
        for (at, c) in token.char_indices() {
            if matches!(c, '=' | '!' | '>' | '<' | ':') {
                let (operator, n) = match token.get(at..at + 2) {
                    Some("!=") => (SearchOperator::NotEqual, 2),
                    Some(">=") => (SearchOperator::GreaterEqual, 2),
                    Some("<=") => (SearchOperator::LessEqual, 2),
                    _ => match c {
                        '=' => (SearchOperator::Equal, 1),
                        ':' => (SearchOperator::Contains, 1),
                        '>' => (SearchOperator::Greater, 1),
                        '<' => (SearchOperator::Less, 1),
                        _ => return Err(invalid("invalid search operator")),
                    },
                };
                let name = &token[..at];
                let value = token
                    .get(at + n..)
                    .ok_or_else(|| invalid("invalid query value"))?;
                let field = match name {
                    "id" | "record_id" => SearchField::RecordId,
                    "path" => SearchField::Path,
                    "mime" => SearchField::Mime,
                    "status" => SearchField::Status,
                    "hash" | "sha256" => SearchField::Sha256,
                    "size" | "size_bytes" => SearchField::Size,
                    "note" => SearchField::Note,
                    "tag" => SearchField::Tag,
                    "raw" | "namespace" => SearchField::Namespace,
                    "conflicts" => SearchField::Conflicts,
                    "text" => SearchField::Text,
                    _ => {
                        let key = name
                            .strip_prefix("claim.")
                            .or_else(|| name.strip_prefix("meta."))
                            .ok_or_else(|| {
                                invalid(
                                    "unknown search field; use claim.<canonical.key> for metadata",
                                )
                            })?;
                        SearchField::Claim(CanonicalKey::parse(key).map_err(io::Error::other)?)
                    }
                };
                let operator = if value == "*"
                    && matches!(operator, SearchOperator::Equal | SearchOperator::Contains)
                {
                    SearchOperator::Exists
                } else {
                    operator
                };
                let value = if matches!(field, SearchField::Status) {
                    match value.to_ascii_lowercase().as_str() {
                        "complete" => "Complete".into(),
                        "partial" => "Partial".into(),
                        "failed" => "Failed".into(),
                        "unsupported" => "Unsupported".into(),
                        _ => value.into(),
                    }
                } else {
                    value.into()
                };
                parsed = Some(SearchPredicate {
                    field,
                    operator,
                    value,
                });
                break;
            }
        }
        let predicate = parsed.unwrap_or_else(|| SearchPredicate {
            field: SearchField::Text,
            operator: SearchOperator::Contains,
            value: token.into(),
        });
        let expression = SearchExpression::Predicate(predicate);
        expressions.push(if negate {
            SearchExpression::Not(Box::new(expression))
        } else {
            expression
        });
    }
    let query = SearchQuery::new(scan_id, SearchExpression::All(expressions));
    query.validate()?;
    Ok(query)
}
