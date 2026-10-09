//! Reproducible lineage for a generated prior-book artifact.
//!
//! The manifest is deliberately separate from the JSONL book header. The
//! header is part of the runtime loading contract, while this sidecar records
//! dataset and adapter lineage that can vary by downstream integration.
//! Keeping them separate lets existing schema-v1 readers continue to load a
//! book even when a newer producer emits richer archival evidence.

use crate::{BuildConfig, BuildStats, Error, PriorBook, Result, build_config_fingerprint};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable identifier for the first prior-artifact manifest contract.
pub const PRIOR_ARTIFACT_MANIFEST_SCHEMA: &str = "lineprior.prior-artifact-manifest.v1";

/// Caller-owned identity for the dataset and adapter that produced a book.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriorArtifactLineage {
    pub dataset_id: Option<String>,
    pub split: Option<String>,
    pub adapter_id: Option<String>,
    pub adapter_version: Option<String>,
    /// Domain-specific identifiers such as a downstream commit or corpus
    /// manifest hash. Values stay strings so the core never interprets a
    /// domain schema.
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

/// Counts that make a built artifact auditable without reopening the book.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriorArtifactSummary {
    pub warning_count: usize,
    pub order_zero_states_kept: usize,
    pub context_entries_kept: usize,
    pub build_stats: BuildStats,
}

/// Sidecar manifest linking exact input bytes to exact prior-book bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriorArtifactManifestV1 {
    pub schema: String,
    pub producer_version: String,
    pub input_sha256: String,
    pub prior_book_sha256: String,
    pub build_config: serde_json::Value,
    pub build_config_fingerprint: u64,
    pub lineage: PriorArtifactLineage,
    pub summary: PriorArtifactSummary,
}

impl PriorArtifactManifestV1 {
    /// Construct a manifest after the input and emitted book have been hashed.
    ///
    /// Hashing remains an I/O concern of the caller. The library validates
    /// that both values are canonical lowercase SHA-256 strings so malformed
    /// lineage cannot look complete.
    pub fn new(
        input_sha256: String,
        prior_book_sha256: String,
        lineage: PriorArtifactLineage,
        book: &PriorBook,
        config: &BuildConfig,
        stats: BuildStats,
        warning_count: usize,
    ) -> Result<Self> {
        validate_sha256("input_sha256", &input_sha256)?;
        validate_sha256("prior_book_sha256", &prior_book_sha256)?;
        Ok(Self {
            schema: PRIOR_ARTIFACT_MANIFEST_SCHEMA.to_string(),
            producer_version: env!("CARGO_PKG_VERSION").to_string(),
            input_sha256,
            prior_book_sha256,
            build_config: serde_json::to_value(config)
                .expect("BuildConfig always serializes to a JSON value"),
            build_config_fingerprint: build_config_fingerprint(config),
            lineage,
            summary: PriorArtifactSummary {
                warning_count,
                order_zero_states_kept: book.entries.len(),
                context_entries_kept: book.context_entries.len(),
                build_stats: stats,
            },
        })
    }
}

fn validate_sha256(field: &'static str, value: &str) -> Result<()> {
    let valid = value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidArtifactDigest {
            field,
            value: value.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_links_input_book_config_and_counts() {
        let config = BuildConfig::default();
        let book = PriorBook::default();
        let manifest = PriorArtifactManifestV1::new(
            "0".repeat(64),
            "a".repeat(64),
            PriorArtifactLineage {
                dataset_id: Some("fixture-v1".to_string()),
                adapter_id: Some("example".to_string()),
                ..Default::default()
            },
            &book,
            &config,
            BuildStats::default(),
            2,
        )
        .unwrap();

        assert_eq!(manifest.schema, PRIOR_ARTIFACT_MANIFEST_SCHEMA);
        assert_eq!(manifest.producer_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(manifest.summary.warning_count, 2);
        assert_eq!(
            manifest.build_config_fingerprint,
            build_config_fingerprint(&config)
        );
    }

    #[test]
    fn manifest_rejects_noncanonical_hashes() {
        let error = PriorArtifactManifestV1::new(
            "not-a-digest".to_string(),
            "a".repeat(64),
            PriorArtifactLineage::default(),
            &PriorBook::default(),
            &BuildConfig::default(),
            BuildStats::default(),
            0,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            Error::InvalidArtifactDigest {
                field: "input_sha256",
                ..
            }
        ));
    }
}
