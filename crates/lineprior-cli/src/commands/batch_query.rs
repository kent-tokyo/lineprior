use super::{exit_code_for_lineprior_error, hashing::HashingReader, same_path};
use anyhow::{Context, Result};
use clap::Args;
use lineprior::{
    BatchQueryConfig, BatchQueryReportHeaderV1, PriorArtifactBookCompatibility,
    PriorArtifactManifestV1, PriorBookMetadata, evaluate_batch_queries,
    load_prior_book_with_metadata, parse_batch_query_jsonl,
};
use serde::Serialize;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Args)]
pub struct BatchQueryArgs {
    /// Frozen schema-v1 prior book JSONL.
    book: PathBuf,

    /// Versioned batch-query input JSONL.
    queries: PathBuf,

    /// Prior-artifact sidecar that must match the exact book bytes.
    #[arg(long)]
    artifact_manifest: PathBuf,

    /// Deterministic JSONL report output.
    #[arg(long)]
    out: PathBuf,

    /// Maximum candidates selected per query.
    #[arg(long, default_value_t = 5)]
    top_k: usize,

    /// Additional minimum raw support applied at query time.
    #[arg(long, default_value_t = 0)]
    min_count: u64,

    /// Additional minimum finite weighted support applied at query time.
    #[arg(long, default_value_t = 0.0)]
    min_weighted_count: f64,

    /// Additional minimum confidence in 0..=1 applied at query time.
    #[arg(long, default_value_t = 0.0)]
    min_confidence: f64,
}

fn open(path: &Path) -> std::io::Result<File> {
    File::open(path)
}

fn load_book_and_sha256(
    reader: impl std::io::Read,
) -> lineprior::Result<(lineprior::LoadedPriorBook, String)> {
    let mut reader = HashingReader::new(reader, true);
    let loaded = load_prior_book_with_metadata(&mut reader)?;
    let digest = reader
        .finish_sha256()
        .expect("hashing is enabled for batch query");
    Ok((loaded, digest))
}

fn load_queries_and_sha256(
    reader: impl std::io::Read,
) -> lineprior::Result<(Vec<lineprior::BatchQueryInputV1>, String)> {
    let mut reader = HashingReader::new(reader, true);
    let queries = parse_batch_query_jsonl(&mut reader)?;
    let digest = reader
        .finish_sha256()
        .expect("hashing is enabled for batch query");
    Ok((queries, digest))
}

fn write_json_line(writer: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *writer, value).context("serializing batch-query report")?;
    writer
        .write_all(b"\n")
        .context("finishing batch-query report row")
}

pub fn run(args: BatchQueryArgs) -> Result<ExitCode> {
    let inputs = [&args.book, &args.queries, &args.artifact_manifest];
    if inputs.iter().any(|input| same_path(input, &args.out)) {
        eprintln!("error: --out must differ from book, queries, and --artifact-manifest");
        return Ok(ExitCode::from(3));
    }

    let config = BatchQueryConfig {
        top_k: args.top_k,
        min_count: args.min_count,
        min_weighted_count: args.min_weighted_count,
        min_confidence: args.min_confidence,
    };
    if let Err(error) = config.validate() {
        eprintln!("error: {error}");
        return Ok(exit_code_for_lineprior_error(&error));
    }

    let manifest_file = match open(&args.artifact_manifest) {
        Ok(file) => file,
        Err(error) => {
            eprintln!(
                "error: opening {}: {error}",
                args.artifact_manifest.display()
            );
            return Ok(ExitCode::from(3));
        }
    };
    let manifest: PriorArtifactManifestV1 =
        match serde_json::from_reader(BufReader::new(manifest_file)) {
            Ok(manifest) => manifest,
            Err(error) => {
                eprintln!(
                    "error: parsing {}: {error}",
                    args.artifact_manifest.display()
                );
                return Ok(ExitCode::from(3));
            }
        };

    let book_file = match open(&args.book) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("error: opening {}: {error}", args.book.display());
            return Ok(ExitCode::from(3));
        }
    };
    let (loaded, prior_book_sha256) = match load_book_and_sha256(book_file) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("error: validating {}: {error}", args.book.display());
            return Ok(exit_code_for_lineprior_error(&error));
        }
    };
    if let Err(error) = manifest.verify_book_digest(&prior_book_sha256) {
        eprintln!("error: {error}");
        return Ok(exit_code_for_lineprior_error(&error));
    }
    let compatibility = match manifest.verify_book_metadata(loaded.metadata.as_ref()) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("error: {error}");
            return Ok(exit_code_for_lineprior_error(&error));
        }
    };
    if compatibility != PriorArtifactBookCompatibility::Versioned {
        eprintln!("error: batch-query requires a manifest-matched schema-v1 prior book");
        return Ok(ExitCode::from(3));
    }
    let metadata = match loaded.metadata.as_ref() {
        Some(PriorBookMetadata::Versioned(metadata)) => metadata,
        _ => {
            eprintln!("error: batch-query requires schema-v1 prior-book metadata");
            return Ok(ExitCode::from(3));
        }
    };

    let query_file = match open(&args.queries) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("error: opening {}: {error}", args.queries.display());
            return Ok(ExitCode::from(3));
        }
    };
    let (queries, query_input_sha256) = match load_queries_and_sha256(query_file) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("error: validating {}: {error}", args.queries.display());
            return Ok(exit_code_for_lineprior_error(&error));
        }
    };

    let results = match evaluate_batch_queries(&loaded.book, &queries, &config) {
        Ok(results) => results,
        Err(error) => {
            eprintln!("error: {error}");
            return Ok(exit_code_for_lineprior_error(&error));
        }
    };
    let header = BatchQueryReportHeaderV1::new(
        query_input_sha256,
        prior_book_sha256,
        &manifest,
        metadata,
        config,
        queries.len(),
    )?;

    let out =
        File::create(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
    let mut writer = BufWriter::new(out);
    write_json_line(&mut writer, &header)?;
    for result in &results {
        write_json_line(&mut writer, result)?;
    }
    writer.flush().context("flushing batch-query report")?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_loader_hashes_the_same_stream_it_parses() {
        let bytes = b"{\"batch_query_schema_version\":1,\"query_id\":\"q\",\"state\":\"s\"}\n";
        let (queries, digest) = load_queries_and_sha256(&bytes[..]).unwrap();
        assert_eq!(queries.len(), 1);
        assert_eq!(
            digest,
            "7756bbe65d5f50d9305187d4b0d06ee22c72c2fef4523af3b9aed0a18a6f39b7"
        );
    }
}
