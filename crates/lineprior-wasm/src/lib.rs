//! Minimal WebAssembly bindings for JSON-in/JSON-out build and query.
//!
//! The Rust `lineprior` crate remains authoritative: this wrapper only
//! translates strings at the JavaScript boundary. It does not expose file
//! I/O, embeddings, or a second scoring implementation.

use lineprior::{BuildConfig, PriorEntry, build_prior_book_from_reader, load_prior_book};
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
struct BuildJsonOutput {
    entries: Vec<PriorEntry>,
    warnings: Vec<String>,
    stats: lineprior::BuildStats,
}

/// Build a prior from JSONL observations and return a JSON result.
///
/// `config_json` is a serialized `lineprior::BuildConfig`. The binding uses
/// non-strict parsing, so recoverable record problems appear in `warnings`;
/// malformed configuration or structural errors return a JavaScript error.
#[wasm_bindgen]
pub fn build_json(input_jsonl: &str, config_json: &str) -> Result<String, JsValue> {
    build_json_inner(input_jsonl, config_json).map_err(|error| JsValue::from_str(&error))
}

fn build_json_inner(input_jsonl: &str, config_json: &str) -> Result<String, String> {
    let config: BuildConfig = serde_json::from_str(config_json)
        .map_err(|error| format!("invalid BuildConfig JSON: {error}"))?;
    let output = build_prior_book_from_reader(input_jsonl.as_bytes(), false, &config)
        .map_err(|error| error.to_string())?;
    let result = BuildJsonOutput {
        entries: output
            .book
            .entries_sorted()
            .into_iter()
            .chain(output.book.context_entries_sorted())
            .collect(),
        warnings: output
            .warnings
            .into_iter()
            .map(|warning| warning.to_string())
            .collect(),
        stats: output.stats,
    };
    serde_json::to_string(&result).map_err(|error| format!("serializing build result: {error}"))
}

/// Query a JSONL prior book and return the ranked candidates as JSON.
///
/// The input uses the native prior-book JSONL format. A missing state returns
/// `[]`, matching the native API; callers can persist the `entries` array from
/// [`build_json`] as one JSON object per line before querying it.
#[wasm_bindgen]
pub fn query_json(prior_jsonl: &str, state: &str, top_k: Option<usize>) -> Result<String, JsValue> {
    query_json_inner(prior_jsonl, state, top_k).map_err(|error| JsValue::from_str(&error))
}

fn query_json_inner(
    prior_jsonl: &str,
    state: &str,
    top_k: Option<usize>,
) -> Result<String, String> {
    let book = load_prior_book(prior_jsonl.as_bytes()).map_err(|error| error.to_string())?;
    let candidates = book.query(state, top_k);
    serde_json::to_string(&candidates).map_err(|error| format!("serializing query result: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn build_json_returns_native_entries_and_stats() {
        let input = r#"{"sequence_id":"s","step":0,"state":"screen","action":"click","outcome":"success"}
{"sequence_id":"s2","step":0,"state":"screen","action":"wait","outcome":"failure"}
"#;
        let result: Value = serde_json::from_str(&build_json_inner(input, "{}").unwrap()).unwrap();
        assert_eq!(result["entries"][0]["state"], "screen");
        assert_eq!(result["stats"]["observations_kept"], 2);
        assert!(result["warnings"].as_array().unwrap().is_empty());
    }

    #[test]
    fn build_json_preserves_non_strict_record_warnings() {
        let input = r#"{"sequence_id":"s","step":0,"state":"screen","action":"click"}
not-json
"#;
        let result: Value = serde_json::from_str(&build_json_inner(input, "{}").unwrap()).unwrap();
        assert_eq!(result["stats"]["observations_kept"], 1);
        assert_eq!(result["warnings"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn query_json_matches_the_native_empty_state_fallback() {
        let prior = r#"{"state":"screen","actions":[{"action":"click","count":1,"weighted_count":1.0,"success_rate":1.0,"mean_score":null,"prior":1.0,"confidence":0.1}]}
"#;
        assert_eq!(query_json_inner(prior, "missing", None).unwrap(), "[]");
        let candidates: Value =
            serde_json::from_str(&query_json_inner(prior, "screen", Some(1)).unwrap()).unwrap();
        assert_eq!(candidates[0]["action"], "click");
    }

    #[test]
    fn build_json_rejects_malformed_config_at_the_boundary() {
        assert!(build_json_inner("", "not-json").is_err());
    }

    #[test]
    fn query_json_rejects_malformed_prior_at_the_boundary() {
        assert!(query_json_inner("not-jsonl", "screen", None).is_err());
    }
}
