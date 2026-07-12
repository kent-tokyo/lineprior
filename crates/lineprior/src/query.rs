use crate::error::{Error, Result};
use crate::model::{BuildConfig, PriorAction, PriorBook, PriorEntry};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};

/// Matches `PriorBook::entries`'s shape.
type Entries = HashMap<String, Vec<PriorAction>>;
/// Matches `PriorBook::context_entries`'s shape.
type ContextEntries = HashMap<(Vec<String>, String), Vec<PriorAction>>;

/// Deserializes one line of a saved prior book.
fn parse_entry_line(line: &str, line_no: usize) -> Result<PriorEntry> {
    serde_json::from_str(line).map_err(|source| Error::Json {
        line: line_no,
        source,
    })
}

/// Optional first line of a book saved via [`save_prior_book_with_config`]:
/// a fingerprint of the `BuildConfig` used to build it. Schema-disjoint
/// from [`PriorEntry`] (each requires a field the other lacks), so a
/// header can never be mistaken for a state entry or vice versa.
#[derive(Serialize, Deserialize)]
struct BookHeader {
    build_config_fingerprint: u64,
}

/// Routes one parsed [`PriorEntry`] into `entries` (order-0, `context`
/// empty) or `context_entries` (order `1..=k`, `context` non-empty) --
/// shared by both branches of [`load_entries`]'s line loop.
fn insert_entry(entries: &mut Entries, context_entries: &mut ContextEntries, entry: PriorEntry) {
    if entry.context.is_empty() {
        entries.insert(entry.state, entry.actions);
    } else {
        context_entries.insert((entry.context, entry.state), entry.actions);
    }
}

/// Shared by [`load_prior_book`] and [`load_prior_book_with_config`]: reads
/// every line, transparently skipping a leading header if present (whether
/// or not the caller cares to validate it), and returns whatever
/// fingerprint it found alongside the parsed entries.
fn load_entries(reader: impl Read) -> Result<(Entries, ContextEntries, Option<u64>)> {
    let mut entries: Entries = HashMap::new();
    let mut context_entries: ContextEntries = HashMap::new();
    let mut fingerprint = None;
    let mut lines = BufReader::new(reader).lines().enumerate();

    if let Some((index, line)) = lines.next() {
        let line = line?;
        if !line.trim().is_empty() {
            match serde_json::from_str::<BookHeader>(&line) {
                Ok(header) => fingerprint = Some(header.build_config_fingerprint),
                Err(_) => {
                    let entry = parse_entry_line(&line, index + 1)?;
                    insert_entry(&mut entries, &mut context_entries, entry);
                }
            }
        }
    }

    for (index, line) in lines {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let entry = parse_entry_line(&line, index + 1)?;
        insert_entry(&mut entries, &mut context_entries, entry);
    }

    Ok((entries, context_entries, fingerprint))
}

/// Reads a prior book back from the JSONL format emitted by `build`.
/// Querying itself is `PriorBook::query` -- an unseen state returns no
/// candidates rather than an error or an invented action.
///
/// Tolerates (and ignores) a leading config-fingerprint header written by
/// [`save_prior_book_with_config`]; use [`load_prior_book_with_config`] to
/// actually validate it against an expected `BuildConfig`.
pub fn load_prior_book(reader: impl Read) -> Result<PriorBook> {
    let (entries, context_entries, _fingerprint) = load_entries(reader)?;
    Ok(PriorBook {
        entries,
        context_entries,
    })
}

/// Writes a prior book as JSONL: order-0 entries in the same deterministic
/// order as [`PriorBook::entries_sorted`], followed by any context entries
/// in [`PriorBook::context_entries_sorted`]'s order. The inverse of
/// [`load_prior_book`].
///
/// Flushes before returning -- a buffered writer's `Drop` swallows flush
/// errors, so a late write failure (e.g. a full disk) must surface here.
pub fn save_prior_book(book: &PriorBook, mut writer: impl Write) -> Result<()> {
    for entry in book
        .entries_sorted()
        .into_iter()
        .chain(book.context_entries_sorted())
    {
        serde_json::to_writer(&mut writer, &entry)
            .map_err(|e| Error::Io(std::io::Error::other(e)))?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    Ok(())
}

/// Deterministic fingerprint of a [`BuildConfig`], stable *within a given
/// lineprior version* (it hashes a JSON encoding, and serde_json's exact
/// byte layout for floats is not itself guaranteed forever-stable across
/// serde_json versions -- unlike `eval`'s sequence-id hash, which only
/// ever hashes raw string bytes and so carries a stronger cross-version
/// guarantee). Good enough to detect a stale cached prior book within one
/// project's lifetime; not meant as a long-term archival checksum.
pub fn build_config_fingerprint(config: &BuildConfig) -> u64 {
    let canonical = serde_json::to_vec(config).expect("BuildConfig always serializes");
    crate::hash::fnv1a(&canonical)
}

/// Like [`save_prior_book`], but also writes a leading header line with
/// `config`'s fingerprint, so a later [`load_prior_book_with_config`] call
/// can detect whether the book was built under different config values
/// than the caller currently expects.
pub fn save_prior_book_with_config(
    book: &PriorBook,
    config: &BuildConfig,
    mut writer: impl Write,
) -> Result<()> {
    let header = BookHeader {
        build_config_fingerprint: build_config_fingerprint(config),
    };
    serde_json::to_writer(&mut writer, &header).map_err(|e| Error::Io(std::io::Error::other(e)))?;
    writer.write_all(b"\n")?;
    save_prior_book(book, writer)
}

/// Like [`load_prior_book`], but also checks the file's embedded
/// `BuildConfig` fingerprint (if any) against `expected_config`'s
/// fingerprint. Returns [`Error::BuildConfigMismatch`] if the file has a
/// header and it doesn't match -- otherwise a stale cached book could
/// silently be reused as if its `confidence`/`prior` values were computed
/// under the caller's current config. A file with no header (saved by
/// plain [`save_prior_book`], or by a version of lineprior that predates
/// this) is accepted unconditionally -- there's nothing to compare against.
pub fn load_prior_book_with_config(
    reader: impl Read,
    expected_config: &BuildConfig,
) -> Result<PriorBook> {
    let (entries, context_entries, fingerprint) = load_entries(reader)?;
    if let Some(found) = fingerprint {
        let expected = build_config_fingerprint(expected_config);
        if found != expected {
            return Err(Error::BuildConfigMismatch { expected, found });
        }
    }
    Ok(PriorBook {
        entries,
        context_entries,
    })
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

    #[test]
    fn save_then_load_round_trips_context_entries() {
        let mut book = sample_book(); // order-0: state "s" -> action "a"
        book.context_entries.insert(
            (vec!["x".to_string()], "s".to_string()),
            vec![PriorAction {
                action: "b".to_string(),
                count: 3,
                weighted_count: 3.0,
                success_rate: Some(1.0),
                mean_score: None,
                prior: 1.0,
                confidence: 0.3,
            }],
        );

        let mut buf: Vec<u8> = Vec::new();
        save_prior_book(&book, &mut buf).unwrap();
        let contents = String::from_utf8(buf.clone()).unwrap();
        assert!(contents.contains("\"context\":[\"x\"]"));

        let reloaded = load_prior_book(buf.as_slice()).unwrap();
        // Order-0 is untouched.
        assert_eq!(reloaded.query("s", None), book.query("s", None));
        // Context entry round-trips too.
        let result = reloaded.query_with_context("s", &["x".to_string()], None);
        assert_eq!(result.matched_order, 1);
        assert_eq!(result.candidates[0].action, "b");
    }

    #[test]
    fn build_config_fingerprint_is_deterministic() {
        let config = BuildConfig::default();
        assert_eq!(
            build_config_fingerprint(&config),
            build_config_fingerprint(&config)
        );
    }

    #[test]
    fn build_config_fingerprint_is_sensitive_to_config_changes() {
        let a = BuildConfig::default();
        let b = BuildConfig {
            smoothing_alpha: a.smoothing_alpha + 1.0,
            ..a.clone()
        };
        assert_ne!(build_config_fingerprint(&a), build_config_fingerprint(&b));
    }

    #[test]
    fn load_prior_book_skips_a_config_header_transparently() {
        let book = sample_book();
        let mut buf: Vec<u8> = Vec::new();
        save_prior_book_with_config(&book, &BuildConfig::default(), &mut buf).unwrap();

        // Plain load_prior_book never validates the header, just tolerates it.
        let reloaded = load_prior_book(buf.as_slice()).unwrap();
        assert_eq!(reloaded.query("s", None), book.query("s", None));
    }

    #[test]
    fn load_prior_book_with_config_succeeds_when_config_matches() {
        let book = sample_book();
        let config = BuildConfig::default();
        let mut buf: Vec<u8> = Vec::new();
        save_prior_book_with_config(&book, &config, &mut buf).unwrap();

        let reloaded = load_prior_book_with_config(buf.as_slice(), &config).unwrap();
        assert_eq!(reloaded.query("s", None), book.query("s", None));
    }

    #[test]
    fn load_prior_book_with_config_errors_when_config_differs() {
        let book = sample_book();
        let saved_with = BuildConfig::default();
        let mut buf: Vec<u8> = Vec::new();
        save_prior_book_with_config(&book, &saved_with, &mut buf).unwrap();

        let expected_now = BuildConfig {
            smoothing_alpha: saved_with.smoothing_alpha + 1.0,
            ..saved_with
        };
        let err = load_prior_book_with_config(buf.as_slice(), &expected_now).unwrap_err();
        assert!(matches!(err, Error::BuildConfigMismatch { .. }));
    }

    #[test]
    fn load_prior_book_with_config_accepts_a_plain_headerless_file() {
        let book = sample_book();
        let mut buf: Vec<u8> = Vec::new();
        save_prior_book(&book, &mut buf).unwrap(); // no header written

        // Nothing to compare against, so any config is "compatible".
        let reloaded =
            load_prior_book_with_config(buf.as_slice(), &BuildConfig::default()).unwrap();
        assert_eq!(reloaded.query("s", None), book.query("s", None));
    }
}
