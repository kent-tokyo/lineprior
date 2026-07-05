use crate::error::{Error, Result, Warning};
use crate::model::{Observation, Outcome};
use serde::Deserialize;
use std::io::{BufRead, BufReader, Read};

/// JSON shape as it appears on the wire, before defaults and validation.
/// `Option<T>` fields are implicitly optional to serde (missing key -> `None`),
/// so this alone gives us "field absent" detection without extra attributes.
#[derive(Deserialize)]
struct RawObservation {
    sequence_id: Option<String>,
    step: Option<u32>,
    state: Option<String>,
    action: Option<String>,
    outcome: Option<String>,
    score: Option<f64>,
    weight: Option<f64>,
    tags: Option<Vec<String>>,
}

/// Everything produced by a parse pass: the valid observations plus any
/// non-fatal issues collected in non-strict mode.
#[derive(Debug, Default)]
pub struct ParseOutcome {
    pub observations: Vec<Observation>,
    pub warnings: Vec<Warning>,
}

/// Applies field defaults and validation rules to one raw record.
///
/// Defaults: `outcome` -> unknown, `score` -> null, `weight` -> 1.0,
/// `tags` -> []. Required: `sequence_id`, `step`, `state`, `action`.
fn build_observation(raw: RawObservation, line: usize) -> Result<Observation> {
    let sequence_id = raw.sequence_id.ok_or(Error::MissingField {
        line,
        field: "sequence_id",
    })?;
    let step = raw.step.ok_or(Error::MissingField {
        line,
        field: "step",
    })?;
    let state = raw.state.ok_or(Error::MissingField {
        line,
        field: "state",
    })?;
    let action = raw.action.ok_or(Error::MissingField {
        line,
        field: "action",
    })?;

    if state.is_empty() {
        return Err(Error::EmptyState { line });
    }
    if action.is_empty() {
        return Err(Error::EmptyAction { line });
    }

    let outcome = match raw.outcome {
        None => Outcome::Unknown,
        Some(value) => Outcome::parse(&value).ok_or(Error::UnsupportedOutcome { line, value })?,
    };

    let score = match raw.score {
        None => None,
        Some(value) if value.is_nan() => return Err(Error::NanScore { line }),
        Some(value) => Some(value),
    };

    let weight = raw.weight.unwrap_or(1.0);
    if weight < 0.0 {
        return Err(Error::NegativeWeight {
            line,
            value: weight,
        });
    }

    Ok(Observation {
        sequence_id,
        step,
        state,
        action,
        outcome,
        score,
        weight,
        tags: raw.tags.unwrap_or_default(),
    })
}

/// Streams JSONL observations from `reader`, one line at a time so memory
/// stays bounded regardless of input size.
///
/// In strict mode, the first invalid record aborts the whole parse. In
/// non-strict mode, invalid records are skipped and recorded as warnings
/// so the caller can decide whether to proceed. Blank lines are ignored.
pub fn parse_jsonl(reader: impl Read, strict: bool) -> Result<ParseOutcome> {
    let mut outcome = ParseOutcome::default();

    for (index, line) in BufReader::new(reader).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let line_no = index + 1;

        let record = serde_json::from_str::<RawObservation>(&line)
            .map_err(|source| Error::Json {
                line: line_no,
                source,
            })
            .and_then(|raw| build_observation(raw, line_no));

        match record {
            Ok(observation) => outcome.observations.push(observation),
            Err(err) if strict => return Err(err),
            Err(err) => outcome.warnings.push(Warning {
                line: line_no,
                message: err.to_string(),
            }),
        }
    }

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str, strict: bool) -> Result<ParseOutcome> {
        parse_jsonl(input.as_bytes(), strict)
    }

    #[test]
    fn parses_valid_jsonl() {
        let input = r#"{"sequence_id":"case-001","step":0,"state":"s","action":"a","outcome":"success","score":0.8,"weight":2.0,"tags":["t"]}"#;
        let outcome = parse(input, true).unwrap();
        assert_eq!(outcome.observations.len(), 1);
        let obs = &outcome.observations[0];
        assert_eq!(obs.sequence_id, "case-001");
        assert_eq!(obs.state, "s");
        assert_eq!(obs.action, "a");
        assert_eq!(obs.outcome, Outcome::Success);
        assert_eq!(obs.score, Some(0.8));
        assert_eq!(obs.weight, 2.0);
        assert_eq!(obs.tags, vec!["t".to_string()]);
    }

    #[test]
    fn rejects_malformed_jsonl_in_strict_mode() {
        let err = parse("{not json}", true).unwrap_err();
        assert!(matches!(err, Error::Json { line: 1, .. }));
    }

    #[test]
    fn defaults_missing_optional_fields() {
        let input = r#"{"sequence_id":"c","step":0,"state":"s","action":"a"}"#;
        let outcome = parse(input, true).unwrap();
        let obs = &outcome.observations[0];
        assert_eq!(obs.outcome, Outcome::Unknown);
        assert_eq!(obs.score, None);
        assert_eq!(obs.weight, 1.0);
        assert!(obs.tags.is_empty());
    }

    #[test]
    fn strict_mode_aborts_on_first_invalid_record() {
        let input = "{\"sequence_id\":\"c\",\"step\":0,\"state\":\"s\",\"action\":\"a\"}\n{\"state\":\"\",\"action\":\"a\",\"sequence_id\":\"c\",\"step\":1}\n";
        let err = parse(input, true).unwrap_err();
        assert!(matches!(err, Error::EmptyState { line: 2 }));
    }

    #[test]
    fn non_strict_mode_skips_invalid_records_and_warns() {
        let input = "{\"sequence_id\":\"c\",\"step\":0,\"state\":\"s\",\"action\":\"a\"}\n{\"state\":\"\",\"action\":\"a\",\"sequence_id\":\"c\",\"step\":1}\n";
        let outcome = parse(input, false).unwrap();
        assert_eq!(outcome.observations.len(), 1);
        assert_eq!(outcome.warnings.len(), 1);
        assert_eq!(outcome.warnings[0].line, 2);
    }

    #[test]
    fn empty_input_yields_no_observations() {
        let outcome = parse("", true).unwrap();
        assert!(outcome.observations.is_empty());
        assert!(outcome.warnings.is_empty());
    }

    #[test]
    fn rejects_negative_weight() {
        let input = r#"{"sequence_id":"c","step":0,"state":"s","action":"a","weight":-1.0}"#;
        let err = parse(input, true).unwrap_err();
        assert!(matches!(err, Error::NegativeWeight { line: 1, .. }));
    }

    #[test]
    fn accepts_zero_weight() {
        let input = r#"{"sequence_id":"c","step":0,"state":"s","action":"a","weight":0.0}"#;
        let outcome = parse(input, true).unwrap();
        assert_eq!(outcome.observations[0].weight, 0.0);
    }

    #[test]
    fn rejects_unsupported_outcome() {
        let input = r#"{"sequence_id":"c","step":0,"state":"s","action":"a","outcome":"win"}"#;
        let err = parse(input, true).unwrap_err();
        assert!(matches!(err, Error::UnsupportedOutcome { line: 1, .. }));
    }

    #[test]
    fn rejects_nan_score_at_the_validation_layer() {
        // NaN cannot be spelled in standard JSON text, so this exercises
        // build_observation directly rather than round-tripping through
        // serde_json::from_str.
        let raw = RawObservation {
            sequence_id: Some("c".into()),
            step: Some(0),
            state: Some("s".into()),
            action: Some("a".into()),
            outcome: None,
            score: Some(f64::NAN),
            weight: None,
            tags: None,
        };
        let err = build_observation(raw, 7).unwrap_err();
        assert!(matches!(err, Error::NanScore { line: 7 }));
    }

    #[test]
    fn allows_duplicate_sequence_ids_across_steps() {
        let input = "{\"sequence_id\":\"c\",\"step\":0,\"state\":\"s\",\"action\":\"a\"}\n{\"sequence_id\":\"c\",\"step\":1,\"state\":\"s2\",\"action\":\"a2\"}\n";
        let outcome = parse(input, true).unwrap();
        assert_eq!(outcome.observations.len(), 2);
        assert_eq!(outcome.observations[0].sequence_id, "c");
        assert_eq!(outcome.observations[1].sequence_id, "c");
    }
}
