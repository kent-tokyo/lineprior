//! Deterministic batch telemetry for exact-match prior queries.
//!
//! The types in this module describe evidence from a frozen prior book. They
//! never execute actions, infer outcomes, or manufacture candidates.

use crate::{
    Error, PriorArtifactManifestV1, PriorBook, PriorBookMetadata, PriorBookMetadataV1,
    QueryScratch, Result,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read};

/// Schema version required on every batch-query input row.
pub const BATCH_QUERY_INPUT_SCHEMA_VERSION: u32 = 1;

/// Schema identifier written on the first line of a batch-query report.
pub const BATCH_QUERY_REPORT_SCHEMA: &str = "lineprior.batch-query-report.v1";

/// One caller-supplied exact-state lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchQueryInputV1 {
    pub batch_query_schema_version: u32,
    pub query_id: String,
    pub state: String,
    /// Optional per-query candidate budget. The report-level top_k remains
    /// an upper bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_budget: Option<usize>,
}

/// Support thresholds and output bound applied uniformly to a batch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchQueryConfig {
    pub top_k: usize,
    pub min_count: u64,
    pub min_weighted_count: f64,
    pub min_confidence: f64,
}

impl Default for BatchQueryConfig {
    fn default() -> Self {
        Self {
            top_k: 5,
            min_count: 0,
            min_weighted_count: 0.0,
            min_confidence: 0.0,
        }
    }
}

impl BatchQueryConfig {
    /// Reject controls that would turn a telemetry run into an ambiguous
    /// zero-budget query or make threshold comparisons non-finite.
    pub fn validate(&self) -> Result<()> {
        if self.top_k == 0 {
            return Err(Error::InvalidBatchQueryConfig {
                message: "top_k must be greater than zero".to_string(),
            });
        }
        if !self.min_weighted_count.is_finite() || self.min_weighted_count < 0.0 {
            return Err(Error::InvalidBatchQueryConfig {
                message: "min_weighted_count must be finite and non-negative".to_string(),
            });
        }
        if !self.min_confidence.is_finite() || !(0.0..=1.0).contains(&self.min_confidence) {
            return Err(Error::InvalidBatchQueryConfig {
                message: "min_confidence must be finite and between 0 and 1".to_string(),
            });
        }
        Ok(())
    }
}

/// Why a batch lookup selected candidates or abstained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchQueryStatus {
    Matched,
    UnseenState,
    InsufficientSupport,
}

/// One ranked candidate copied into the stable report boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchQueryCandidate {
    pub rank: usize,
    pub action: String,
    pub count: u64,
    pub weighted_count: f64,
    pub prior: f64,
    pub confidence: f64,
}

/// First JSONL record linking the query rows to exact input and book bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchQueryReportHeaderV1 {
    pub record_type: String,
    pub schema: String,
    pub producer_version: String,
    pub query_input_sha256: String,
    pub prior_book_sha256: String,
    pub prior_artifact_input_sha256: String,
    pub prior_artifact_manifest_schema: String,
    pub prior_book_schema_version: u32,
    pub prior_book_producer_version: String,
    pub build_config_fingerprint: u64,
    pub query_config: BatchQueryConfig,
    pub query_count: usize,
}

impl BatchQueryReportHeaderV1 {
    /// Construct a header after validating the query digest and verifying the
    /// manifest against the exact book digest and schema-v1 metadata.
    pub fn new(
        query_input_sha256: String,
        prior_book_sha256: String,
        manifest: &PriorArtifactManifestV1,
        metadata: &PriorBookMetadataV1,
        query_config: BatchQueryConfig,
        query_count: usize,
    ) -> Result<Self> {
        query_config.validate()?;
        crate::artifact::validate_sha256("query_input_sha256", &query_input_sha256)?;
        manifest.verify_book_digest(&prior_book_sha256)?;
        manifest.verify_book_metadata(Some(&PriorBookMetadata::Versioned(metadata.clone())))?;
        Ok(Self {
            record_type: "header".to_string(),
            schema: BATCH_QUERY_REPORT_SCHEMA.to_string(),
            producer_version: env!("CARGO_PKG_VERSION").to_string(),
            query_input_sha256,
            prior_book_sha256,
            prior_artifact_input_sha256: manifest.input_sha256.clone(),
            prior_artifact_manifest_schema: manifest.schema.clone(),
            prior_book_schema_version: metadata.schema_version,
            prior_book_producer_version: metadata.producer_version.clone(),
            build_config_fingerprint: metadata.build_config_fingerprint,
            query_config,
            query_count,
        })
    }
}

/// One output record for one input query, in the same order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchQueryResultV1 {
    pub record_type: String,
    pub query_id: String,
    pub state: String,
    pub status: BatchQueryStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_budget: Option<usize>,
    pub applied_top_k: usize,
    pub available_action_count: usize,
    pub eligible_action_count: usize,
    pub selected_count: usize,
    pub truncated: bool,
    pub candidates: Vec<BatchQueryCandidate>,
}

/// Parse and validate all batch-query rows.
///
/// Validation completes before a CLI output file is created, so malformed,
/// future-version, duplicate-id, or zero-budget rows fail closed.
pub fn parse_batch_query_jsonl(reader: impl Read) -> Result<Vec<BatchQueryInputV1>> {
    let mut queries = Vec::new();
    let mut query_ids = HashSet::new();
    for (index, line) in BufReader::new(reader).lines().enumerate() {
        let line_number = index + 1;
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let query: BatchQueryInputV1 =
            serde_json::from_str(&line).map_err(|source| Error::Json {
                line: line_number,
                source,
            })?;
        if query.batch_query_schema_version != BATCH_QUERY_INPUT_SCHEMA_VERSION {
            return Err(Error::UnsupportedBatchQuerySchemaVersion {
                line: line_number,
                found: query.batch_query_schema_version,
                supported: BATCH_QUERY_INPUT_SCHEMA_VERSION,
            });
        }
        if query.query_id.trim().is_empty() {
            return Err(Error::InvalidBatchQueryRow {
                line: line_number,
                message: "query_id must not be empty".to_string(),
            });
        }
        if query.state.trim().is_empty() {
            return Err(Error::InvalidBatchQueryRow {
                line: line_number,
                message: "state must not be empty".to_string(),
            });
        }
        if query.candidate_budget == Some(0) {
            return Err(Error::InvalidBatchQueryRow {
                line: line_number,
                message: "candidate_budget must be greater than zero".to_string(),
            });
        }
        if !query_ids.insert(query.query_id.clone()) {
            return Err(Error::InvalidBatchQueryRow {
                line: line_number,
                message: format!("duplicate query_id '{}'", query.query_id),
            });
        }
        queries.push(query);
    }
    Ok(queries)
}

/// Query a frozen book with one reusable scratch allocation.
pub fn evaluate_batch_queries(
    book: &PriorBook,
    queries: &[BatchQueryInputV1],
    config: &BatchQueryConfig,
) -> Result<Vec<BatchQueryResultV1>> {
    config.validate()?;
    let mut scratch = QueryScratch::new();
    let mut results = Vec::with_capacity(queries.len());

    for query in queries {
        let state_actions = book.entries.get(&query.state);
        let available_action_count = state_actions.map_or(0, Vec::len);
        let applied_top_k = query
            .candidate_budget
            .unwrap_or(config.top_k)
            .min(config.top_k);

        let mut eligible_action_count = 0usize;
        let mut candidates = Vec::with_capacity(applied_top_k.min(available_action_count));
        for action in book.query_borrowed(&query.state, None, &mut scratch) {
            if action.count < config.min_count
                || action.weighted_count < config.min_weighted_count
                || action.confidence < config.min_confidence
            {
                continue;
            }
            eligible_action_count += 1;
            if candidates.len() < applied_top_k {
                candidates.push(BatchQueryCandidate {
                    rank: candidates.len() + 1,
                    action: action.action.clone(),
                    count: action.count,
                    weighted_count: action.weighted_count,
                    prior: action.prior,
                    confidence: action.confidence,
                });
            }
        }

        let status = if !candidates.is_empty() {
            BatchQueryStatus::Matched
        } else if state_actions.is_some() {
            BatchQueryStatus::InsufficientSupport
        } else {
            BatchQueryStatus::UnseenState
        };
        let selected_count = candidates.len();
        results.push(BatchQueryResultV1 {
            record_type: "result".to_string(),
            query_id: query.query_id.clone(),
            state: query.state.clone(),
            status,
            candidate_budget: query.candidate_budget,
            applied_top_k,
            available_action_count,
            eligible_action_count,
            selected_count,
            truncated: eligible_action_count > selected_count,
            candidates,
        });
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BuildConfig, BuildStats, PriorAction, PriorArtifactLineage, build_config_fingerprint,
    };
    use std::collections::HashMap;

    fn book() -> PriorBook {
        PriorBook {
            entries: HashMap::from([
                (
                    "known".to_string(),
                    vec![
                        PriorAction {
                            action: "a".to_string(),
                            count: 10,
                            weighted_count: 10.0,
                            success_rate: Some(0.8),
                            mean_score: None,
                            prior: 0.7,
                            confidence: 0.8,
                        },
                        PriorAction {
                            action: "b".to_string(),
                            count: 2,
                            weighted_count: 2.0,
                            success_rate: Some(0.5),
                            mean_score: None,
                            prior: 0.3,
                            confidence: 0.2,
                        },
                    ],
                ),
                ("filtered".to_string(), Vec::new()),
            ]),
            context_entries: HashMap::new(),
        }
    }

    #[test]
    fn parser_rejects_future_duplicate_and_zero_budget_rows() {
        let future = br#"{"batch_query_schema_version":2,"query_id":"q","state":"s"}"#;
        assert!(matches!(
            parse_batch_query_jsonl(&future[..]),
            Err(Error::UnsupportedBatchQuerySchemaVersion { .. })
        ));

        let duplicate = concat!(
            "{\"batch_query_schema_version\":1,\"query_id\":\"q\",\"state\":\"a\"}\n",
            "{\"batch_query_schema_version\":1,\"query_id\":\"q\",\"state\":\"b\"}\n"
        );
        assert!(matches!(
            parse_batch_query_jsonl(duplicate.as_bytes()),
            Err(Error::InvalidBatchQueryRow { line: 2, .. })
        ));

        let zero =
            br#"{"batch_query_schema_version":1,"query_id":"q","state":"s","candidate_budget":0}"#;
        assert!(matches!(
            parse_batch_query_jsonl(&zero[..]),
            Err(Error::InvalidBatchQueryRow { .. })
        ));
    }

    #[test]
    fn batch_preserves_order_budget_and_abstention_reason() {
        let queries = vec![
            BatchQueryInputV1 {
                batch_query_schema_version: 1,
                query_id: "q1".to_string(),
                state: "known".to_string(),
                candidate_budget: Some(1),
            },
            BatchQueryInputV1 {
                batch_query_schema_version: 1,
                query_id: "q2".to_string(),
                state: "filtered".to_string(),
                candidate_budget: None,
            },
            BatchQueryInputV1 {
                batch_query_schema_version: 1,
                query_id: "q3".to_string(),
                state: "unseen".to_string(),
                candidate_budget: None,
            },
        ];
        let results =
            evaluate_batch_queries(&book(), &queries, &BatchQueryConfig::default()).unwrap();

        assert_eq!(
            results
                .iter()
                .map(|result| result.query_id.as_str())
                .collect::<Vec<_>>(),
            vec!["q1", "q2", "q3"]
        );
        assert_eq!(results[0].candidates[0].action, "a");
        assert_eq!(results[0].selected_count, 1);
        assert!(results[0].truncated);
        assert_eq!(results[1].status, BatchQueryStatus::InsufficientSupport);
        assert_eq!(results[2].status, BatchQueryStatus::UnseenState);
    }

    #[test]
    fn thresholds_abstain_without_inventing_actions() {
        let queries = vec![BatchQueryInputV1 {
            batch_query_schema_version: 1,
            query_id: "q".to_string(),
            state: "known".to_string(),
            candidate_budget: None,
        }];
        let config = BatchQueryConfig {
            min_confidence: 0.9,
            ..Default::default()
        };
        let results = evaluate_batch_queries(&book(), &queries, &config).unwrap();
        assert_eq!(results[0].status, BatchQueryStatus::InsufficientSupport);
        assert!(results[0].candidates.is_empty());
    }

    #[test]
    fn config_rejects_zero_budget_and_invalid_thresholds() {
        for config in [
            BatchQueryConfig {
                top_k: 0,
                ..Default::default()
            },
            BatchQueryConfig {
                min_weighted_count: f64::NAN,
                ..Default::default()
            },
            BatchQueryConfig {
                min_confidence: 1.01,
                ..Default::default()
            },
        ] {
            assert!(matches!(
                config.validate(),
                Err(Error::InvalidBatchQueryConfig { .. })
            ));
        }
    }

    #[test]
    fn report_header_rejects_invalid_query_hash_and_provenance_drift() {
        let config = BuildConfig::default();
        let manifest = PriorArtifactManifestV1::new(
            "0".repeat(64),
            "a".repeat(64),
            PriorArtifactLineage::default(),
            &PriorBook::default(),
            &config,
            BuildStats::default(),
            0,
        )
        .unwrap();
        let metadata = PriorBookMetadataV1 {
            schema_version: crate::PRIOR_BOOK_SCHEMA_VERSION,
            producer_version: env!("CARGO_PKG_VERSION").to_string(),
            build_config: serde_json::to_value(&config).unwrap(),
            build_config_fingerprint: build_config_fingerprint(&config),
        };

        assert!(matches!(
            BatchQueryReportHeaderV1::new(
                "not-a-digest".to_string(),
                "a".repeat(64),
                &manifest,
                &metadata,
                BatchQueryConfig::default(),
                0,
            ),
            Err(Error::InvalidArtifactDigest {
                field: "query_input_sha256",
                ..
            })
        ));
        assert!(matches!(
            BatchQueryReportHeaderV1::new(
                "b".repeat(64),
                "c".repeat(64),
                &manifest,
                &metadata,
                BatchQueryConfig::default(),
                0,
            ),
            Err(Error::ArtifactDigestMismatch {
                field: "prior_book_sha256",
                ..
            })
        ));
    }
}
