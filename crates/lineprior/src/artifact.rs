//! Reproducible lineage for a generated prior-book artifact.
//!
//! The manifest is deliberately separate from the JSONL book header. The
//! header is part of the runtime loading contract, while this sidecar records
//! dataset and adapter lineage that can vary by downstream integration.
//! Keeping them separate lets existing schema-v1 readers continue to load a
//! book even when a newer producer emits richer archival evidence.

use crate::{
    BuildConfig, BuildStats, Error, PriorBook, PriorBookMetadata, Result, build_config_fingerprint,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable identifier for the first prior-artifact manifest contract.
pub const PRIOR_ARTIFACT_MANIFEST_SCHEMA: &str = "lineprior.prior-artifact-manifest.v1";

/// How much prior-book metadata could be cross-checked with a manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PriorArtifactBookCompatibility {
    /// A schema-v1 header matched producer, config, and fingerprint.
    Versioned,
    /// Only the historical config fingerprint was available and matched.
    LegacyFingerprint,
    /// The book had no metadata header; only its byte digest was verifiable.
    Headerless,
}

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

    /// Validate the manifest's schema, digests, and embedded build config.
    ///
    /// The returned config is the normalized current-version representation
    /// used to recompute the manifest fingerprint.
    pub fn validate(&self) -> Result<BuildConfig> {
        if self.schema != PRIOR_ARTIFACT_MANIFEST_SCHEMA {
            return Err(Error::UnsupportedArtifactManifestSchema {
                found: self.schema.clone(),
                supported: PRIOR_ARTIFACT_MANIFEST_SCHEMA,
            });
        }
        validate_sha256("input_sha256", &self.input_sha256)?;
        validate_sha256("prior_book_sha256", &self.prior_book_sha256)?;

        let config: BuildConfig = serde_json::from_value(self.build_config.clone())
            .map_err(|source| Error::InvalidArtifactBuildConfig { source })?;
        crate::build::validate_config(&config)?;
        let expected = build_config_fingerprint(&config);
        if self.build_config_fingerprint != expected {
            return Err(Error::ArtifactBuildConfigFingerprintMismatch {
                expected,
                found: self.build_config_fingerprint,
            });
        }
        Ok(config)
    }

    /// Compare hashes computed from the exact input and prior-book bytes.
    pub fn verify_digests(&self, input_sha256: &str, prior_book_sha256: &str) -> Result<()> {
        self.validate()?;
        verify_digest("input_sha256", &self.input_sha256, input_sha256)?;
        verify_digest(
            "prior_book_sha256",
            &self.prior_book_sha256,
            prior_book_sha256,
        )
    }

    /// Cross-check the prior-book header against this manifest.
    ///
    /// Legacy and headerless books remain readable, but callers can use the
    /// returned compatibility level to avoid treating them as complete
    /// provenance evidence.
    pub fn verify_book_metadata(
        &self,
        metadata: Option<&PriorBookMetadata>,
    ) -> Result<PriorArtifactBookCompatibility> {
        self.validate()?;
        match metadata {
            Some(PriorBookMetadata::Versioned(header)) => {
                verify_metadata(
                    "producer_version",
                    &self.producer_version,
                    &header.producer_version,
                )?;
                if self.build_config != header.build_config {
                    return Err(Error::ArtifactBookMetadataMismatch {
                        field: "build_config",
                        expected: self.build_config.to_string(),
                        found: header.build_config.to_string(),
                    });
                }
                verify_metadata(
                    "build_config_fingerprint",
                    &self.build_config_fingerprint.to_string(),
                    &header.build_config_fingerprint.to_string(),
                )?;
                Ok(PriorArtifactBookCompatibility::Versioned)
            }
            Some(PriorBookMetadata::LegacyFingerprint {
                build_config_fingerprint,
            }) => {
                verify_metadata(
                    "build_config_fingerprint",
                    &self.build_config_fingerprint.to_string(),
                    &build_config_fingerprint.to_string(),
                )?;
                Ok(PriorArtifactBookCompatibility::LegacyFingerprint)
            }
            None => Ok(PriorArtifactBookCompatibility::Headerless),
        }
    }
}

fn verify_digest(field: &'static str, expected: &str, found: &str) -> Result<()> {
    validate_sha256(field, found)?;
    if expected == found {
        Ok(())
    } else {
        Err(Error::ArtifactDigestMismatch {
            field,
            expected: expected.to_string(),
            found: found.to_string(),
        })
    }
}

fn verify_metadata(field: &'static str, expected: &str, found: &str) -> Result<()> {
    if expected == found {
        Ok(())
    } else {
        Err(Error::ArtifactBookMetadataMismatch {
            field,
            expected: expected.to_string(),
            found: found.to_string(),
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

    #[test]
    fn validation_rejects_schema_and_fingerprint_changes() {
        let config = BuildConfig::default();
        let mut manifest = PriorArtifactManifestV1::new(
            "0".repeat(64),
            "a".repeat(64),
            PriorArtifactLineage::default(),
            &PriorBook::default(),
            &config,
            BuildStats::default(),
            0,
        )
        .unwrap();

        manifest.schema = "lineprior.prior-artifact-manifest.v2".to_string();
        assert!(matches!(
            manifest.validate(),
            Err(Error::UnsupportedArtifactManifestSchema { .. })
        ));

        manifest.schema = PRIOR_ARTIFACT_MANIFEST_SCHEMA.to_string();
        manifest.build_config_fingerprint ^= 1;
        assert!(matches!(
            manifest.validate(),
            Err(Error::ArtifactBuildConfigFingerprintMismatch { .. })
        ));
    }

    #[test]
    fn validation_rejects_deserialized_invalid_digest_and_build_config() {
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

        let mut invalid_digest = manifest.clone();
        invalid_digest.input_sha256 = "A".repeat(64);
        assert!(matches!(
            invalid_digest.validate(),
            Err(Error::InvalidArtifactDigest {
                field: "input_sha256",
                ..
            })
        ));

        let mut malformed_config = manifest.clone();
        malformed_config.build_config = serde_json::json!({"min_count": "one"});
        assert!(matches!(
            malformed_config.validate(),
            Err(Error::InvalidArtifactBuildConfig { .. })
        ));

        let invalid_config = BuildConfig {
            min_confidence: 2.0,
            ..config
        };
        let mut semantically_invalid = manifest;
        semantically_invalid.build_config = serde_json::to_value(&invalid_config).unwrap();
        semantically_invalid.build_config_fingerprint = build_config_fingerprint(&invalid_config);
        assert!(matches!(
            semantically_invalid.validate(),
            Err(Error::InvalidConfig { .. })
        ));
    }

    #[test]
    fn validation_rejects_changed_build_config_with_stale_fingerprint() {
        let config = BuildConfig::default();
        let mut manifest = PriorArtifactManifestV1::new(
            "0".repeat(64),
            "a".repeat(64),
            PriorArtifactLineage::default(),
            &PriorBook::default(),
            &config,
            BuildStats::default(),
            0,
        )
        .unwrap();

        manifest.build_config["min_count"] = serde_json::json!(config.min_count + 1);
        assert!(matches!(
            manifest.validate(),
            Err(Error::ArtifactBuildConfigFingerprintMismatch { .. })
        ));
    }

    #[test]
    fn verification_rejects_changed_bytes_and_header() {
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
        assert!(matches!(
            manifest.verify_digests(&"1".repeat(64), &"a".repeat(64)),
            Err(Error::ArtifactDigestMismatch {
                field: "input_sha256",
                ..
            })
        ));

        let metadata = PriorBookMetadata::Versioned(crate::PriorBookMetadataV1 {
            schema_version: crate::PRIOR_BOOK_SCHEMA_VERSION,
            producer_version: env!("CARGO_PKG_VERSION").to_string(),
            build_config: serde_json::to_value(&config).unwrap(),
            build_config_fingerprint: manifest.build_config_fingerprint ^ 1,
        });
        assert!(matches!(
            manifest.verify_book_metadata(Some(&metadata)),
            Err(Error::ArtifactBookMetadataMismatch {
                field: "build_config_fingerprint",
                ..
            })
        ));
    }
}
