use super::BuildConfigArgs;
use anyhow::{Context, Result};
use clap::Args;
use lineprior::{
    PriorArtifactLineage, PriorArtifactManifestV1, build_prior_book_from_reader,
    save_prior_book_with_config,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Args)]
pub struct BuildArgs {
    /// Input JSONL observation log.
    input: PathBuf,

    /// Output path for the prior book JSONL.
    #[arg(long)]
    out: PathBuf,

    /// Load BuildConfig from this JSON file (e.g. from `tune
    /// --save-best-config`) instead of the individual flags below. Errors
    /// if combined with any of them.
    #[arg(long = "config")]
    config_file: Option<PathBuf>,

    #[command(flatten)]
    config: BuildConfigArgs,

    /// Fail on the first invalid record instead of skipping it with a warning.
    #[arg(long)]
    strict: bool,

    /// Write a deterministic sidecar linking input bytes to prior-book bytes.
    #[arg(long)]
    artifact_manifest: Option<PathBuf>,

    /// Caller-owned dataset identifier recorded in --artifact-manifest.
    #[arg(long, requires = "artifact_manifest", value_parser = parse_non_empty)]
    dataset_id: Option<String>,

    /// Caller-owned split name recorded in --artifact-manifest.
    #[arg(long, requires = "artifact_manifest", value_parser = parse_non_empty)]
    split: Option<String>,

    /// Domain adapter identifier recorded in --artifact-manifest.
    #[arg(long, requires = "artifact_manifest", value_parser = parse_non_empty)]
    adapter_id: Option<String>,

    /// Domain adapter version recorded in --artifact-manifest.
    #[arg(long, requires = "artifact_manifest", value_parser = parse_non_empty)]
    adapter_version: Option<String>,

    /// Additional caller-owned lineage in key=value form; repeat as needed.
    #[arg(
        long = "lineage",
        value_parser = parse_lineage,
        action = clap::ArgAction::Append,
        requires = "artifact_manifest"
    )]
    lineage: Vec<(String, String)>,
}

struct HashingReader<R> {
    inner: R,
    hasher: Option<Sha256>,
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        if let Some(hasher) = &mut self.hasher {
            hasher.update(&buffer[..read]);
        }
        Ok(read)
    }
}

struct HashingWriter<W> {
    inner: W,
    hasher: Option<Sha256>,
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buffer)?;
        if let Some(hasher) = &mut self.hasher {
            hasher.update(&buffer[..written]);
        }
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn digest(hasher: Option<Sha256>) -> Option<String> {
    hasher.map(|hasher| format!("{:x}", hasher.finalize()))
}

fn parse_lineage(value: &str) -> std::result::Result<(String, String), String> {
    let (key, value) = value
        .split_once('=')
        .ok_or_else(|| "expected key=value".to_string())?;
    if key.is_empty() || value.is_empty() {
        return Err("lineage key and value must not be empty".to_string());
    }
    Ok((key.to_string(), value.to_string()))
}

fn parse_non_empty(value: &str) -> std::result::Result<String, String> {
    if value.is_empty() {
        Err("value must not be empty".to_string())
    } else {
        Ok(value.to_string())
    }
}

fn collect_lineage(
    entries: Vec<(String, String)>,
) -> std::result::Result<BTreeMap<String, String>, String> {
    let mut metadata = BTreeMap::new();
    for (key, value) in entries {
        if metadata.insert(key.clone(), value).is_some() {
            return Err(format!("duplicate lineage key `{key}`"));
        }
    }
    Ok(metadata)
}

pub fn run(args: BuildArgs) -> Result<ExitCode> {
    if same_path(&args.input, &args.out) {
        eprintln!("error: input and --out must be different files");
        return Ok(ExitCode::from(3));
    }
    if let Some(manifest) = &args.artifact_manifest
        && (same_path(manifest, &args.input) || same_path(manifest, &args.out))
    {
        eprintln!("error: --artifact-manifest must differ from input and --out");
        return Ok(ExitCode::from(3));
    }
    let file = match File::open(&args.input) {
        Ok(f) => f,
        Err(err) => {
            eprintln!("error: opening {}: {err}", args.input.display());
            return Ok(ExitCode::from(3));
        }
    };
    let emit_manifest = args.artifact_manifest.is_some();
    let lineage_metadata = match collect_lineage(args.lineage) {
        Ok(metadata) => metadata,
        Err(message) => {
            eprintln!("error: {message}");
            return Ok(ExitCode::from(3));
        }
    };
    let mut input = HashingReader {
        inner: file,
        hasher: emit_manifest.then(Sha256::new),
    };

    let config = super::resolve_build_config(args.config_file.as_deref(), args.config)?;

    // Streams straight from the file into the prior book -- memory stays
    // bounded by unique (state, action) pairs, not the number of lines
    // read, instead of collecting every observation into a Vec first.
    let output = match build_prior_book_from_reader(&mut input, args.strict, &config) {
        Ok(output) => output,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(super::exit_code_for_lineprior_error(&err));
        }
    };
    let input_sha256 = digest(input.hasher);
    for warning in &output.warnings {
        eprintln!("warning: {warning}");
    }

    if output.book.entries.is_empty() {
        eprintln!("error: no usable data");
        return Ok(ExitCode::from(2));
    }

    let stats = &output.stats;
    eprintln!(
        "stats: {}/{} observations kept, {}/{} candidates kept \
         ({} by min_count, {} by min_weighted_count, {} by min_confidence, {} by max_actions_per_state)",
        stats.observations_kept,
        stats.observations_kept
            + stats.observations_dropped_by_step_or_tag_filter
            + stats.observations_dropped_by_missing_timestamp,
        stats.candidates_kept,
        stats.candidates_before_filtering,
        stats.candidates_dropped_by_min_count,
        stats.candidates_dropped_by_min_weighted_count,
        stats.candidates_dropped_by_min_confidence,
        stats.candidates_dropped_by_max_actions_per_state,
    );

    let out_file =
        File::create(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
    let mut book_writer = HashingWriter {
        inner: BufWriter::new(out_file),
        hasher: emit_manifest.then(Sha256::new),
    };
    save_prior_book_with_config(&output.book, &config, &mut book_writer)
        .context("writing prior book")?;

    if let Some(manifest_path) = args.artifact_manifest {
        let manifest = PriorArtifactManifestV1::new(
            input_sha256.expect("enabled alongside artifact manifest"),
            digest(book_writer.hasher).expect("enabled alongside artifact manifest"),
            PriorArtifactLineage {
                dataset_id: args.dataset_id,
                split: args.split,
                adapter_id: args.adapter_id,
                adapter_version: args.adapter_version,
                metadata: lineage_metadata,
            },
            &output.book,
            &config,
            output.stats,
            output.warnings.len(),
        )?;
        let mut file = BufWriter::new(
            File::create(&manifest_path)
                .with_context(|| format!("creating {}", manifest_path.display()))?,
        );
        serde_json::to_writer_pretty(&mut file, &manifest)
            .context("writing prior artifact manifest")?;
        file.write_all(b"\n")
            .context("finishing prior artifact manifest")?;
        file.flush().context("flushing prior artifact manifest")?;
    }

    if !output.warnings.is_empty() {
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::from(0))
}

fn same_path(left: &std::path::Path, right: &std::path::Path) -> bool {
    left == right
        || match (left.canonicalize(), right.canonicalize()) {
            (Ok(left), Ok(right)) => left == right,
            _ => false,
        }
}
