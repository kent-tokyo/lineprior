use crate::error::{Error, Result};
use crate::model::{PriorBook, PriorEntry};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};

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

/// Writes a prior book as JSONL, one line per state, in the same
/// deterministic order as [`PriorBook::entries_sorted`]. The inverse of
/// [`load_prior_book`].
///
/// Flushes before returning -- a buffered writer's `Drop` swallows flush
/// errors, so a late write failure (e.g. a full disk) must surface here.
pub fn save_prior_book(book: &PriorBook, mut writer: impl Write) -> Result<()> {
    for entry in book.entries_sorted() {
        serde_json::to_writer(&mut writer, &entry)
            .map_err(|e| Error::Io(std::io::Error::other(e)))?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    Ok(())
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

    #[test]
    fn save_then_load_round_trips_a_prior_book() {
        let book = sample_book();
        let mut buf: Vec<u8> = Vec::new();
        save_prior_book(&book, &mut buf).unwrap();

        let reloaded = load_prior_book(buf.as_slice()).unwrap();
        assert_eq!(reloaded.query("s", None), book.query("s", None));
    }
}
