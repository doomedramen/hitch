//! Helpers shared by tests that assert on `--json` documents.

/// Every capitalised token in the document, sorted and deduplicated.
///
/// The P7 decision is that every enum in this envelope is `snake_case`,
/// because a variant's Rust name is not a wire contract anyone should be
/// depending on. A collector over the whole document — rather than an
/// assertion on the four enums this file knows about — is what makes a *new*
/// enum without the rename fail here too.
///
/// `skip_keys` names free-text fields (people's names, SHAs, prose) whose
/// values are not enum spellings and may legitimately be capitalised.
pub fn pascal_case_tokens(value: &serde_json::Value, skip_keys: &[&str], into: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => {
            for token in s.split(['_', '-', ' ']) {
                if token.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                    into.push(token.to_string());
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                pascal_case_tokens(item, skip_keys, into);
            }
        }
        serde_json::Value::Object(fields) => {
            for (key, field) in fields {
                if !skip_keys.contains(&key.as_str()) {
                    pascal_case_tokens(field, skip_keys, into);
                }
                if key.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                    into.push(key.clone());
                }
            }
        }
        _ => {}
    }
}
