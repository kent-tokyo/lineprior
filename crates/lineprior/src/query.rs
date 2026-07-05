use crate::error::{Error, Result};
use crate::model::{PriorBook, PriorEntry};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};

/// Reads a prior book back from the JSONL format emitted by `build`.
/// Querying itself is `PriorBook::query` -- an unseen state returns no
/// candidates rather than an error or an invented action.
pub fn load_prior_book(reader: impl Read) -> Result<PriorBook> {
    let mut entries: HashMap<String, Vec<crate::model::PriorAction>> = HashMap::new();

    for (index, line) in BufReader::new(reader).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: PriorEntry = serde_json::from_str(&line).map_err(|source| Error::Json {
            line: index + 1,
            source,
        })?;
        entries.insert(entry.state, entry.actions);
    }

    Ok(PriorBook { entries })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_book() -> PriorBook {
        let jsonl = concat!(
            r#"{"state":"s","actions":[{"action":"a","count":10,"weighted_count":10.0,"success_rate":0.7,"mean_score":0.6,"prior":0.8,"confidence":0.6}]}"#,
            "\n"
        );
        load_prior_book(jsonl.as_bytes()).unwrap()
    }

    #[test]
    fn loads_prior_jsonl_and_queries_known_state() {
        let book = sample_book();
        let candidates = book.query("s", None);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].action, "a");
    }

    #[test]
    fn query_unseen_state_returns_empty() {
        let book = sample_book();
        assert!(book.query("nonexistent", None).is_empty());
    }

    #[test]
    fn query_honors_top_k() {
        let jsonl = concat!(
            r#"{"state":"s","actions":[{"action":"a","count":1,"weighted_count":1.0,"success_rate":null,"mean_score":null,"prior":0.6,"confidence":0.1},{"action":"b","count":1,"weighted_count":1.0,"success_rate":null,"mean_score":null,"prior":0.4,"confidence":0.1}]}"#,
            "\n"
        );
        let book = load_prior_book(jsonl.as_bytes()).unwrap();
        let top1 = book.query("s", Some(1));
        assert_eq!(top1.len(), 1);
        assert_eq!(top1[0].action, "a");
    }
}
