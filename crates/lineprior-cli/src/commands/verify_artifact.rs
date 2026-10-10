use anyhow::Result;
use clap::Args;
use lineprior::{
    PriorArtifactBookCompatibility, PriorArtifactManifestV1, load_prior_book_with_metadata,
};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Args)]
pub struct VerifyArtifactArgs {
    /// Prior-artifact sidecar written by `lineprior build --artifact-manifest`.
    #[arg(long)]
    manifest: PathBuf,
    /// Exact observation JSONL bytes used to build the prior book.
    #[arg(long)]
    input: PathBuf,
    /// Exact prior-book JSONL bytes linked by the manifest.
    #[arg(long)]
    book: PathBuf,
}

fn open(path: &Path) -> std::io::Result<File> {
    File::open(path)
}

fn sha256(reader: impl Read) -> std::io::Result<String> {
    let mut reader = BufReader::new(reader);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn run(args: VerifyArtifactArgs) -> Result<ExitCode> {
    let manifest_file = match open(&args.manifest) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("error: opening {}: {error}", args.manifest.display());
            return Ok(ExitCode::from(3));
        }
    };
    let manifest: PriorArtifactManifestV1 =
        match serde_json::from_reader(BufReader::new(manifest_file)) {
            Ok(manifest) => manifest,
            Err(error) => {
                eprintln!("error: parsing {}: {error}", args.manifest.display());
                return Ok(ExitCode::from(3));
            }
        };
    if let Err(error) = manifest.validate() {
        eprintln!("error: {error}");
        return Ok(super::exit_code_for_lineprior_error(&error));
    }

    let input_sha256 = match open(&args.input).and_then(sha256) {
        Ok(digest) => digest,
        Err(error) => {
            eprintln!("error: reading {}: {error}", args.input.display());
            return Ok(ExitCode::from(3));
        }
    };
    let book_sha256 = match open(&args.book).and_then(sha256) {
        Ok(digest) => digest,
        Err(error) => {
            eprintln!("error: reading {}: {error}", args.book.display());
            return Ok(ExitCode::from(3));
        }
    };
    if let Err(error) = manifest.verify_digests(&input_sha256, &book_sha256) {
        eprintln!("error: {error}");
        return Ok(super::exit_code_for_lineprior_error(&error));
    }

    let loaded = match open(&args.book).and_then(|file| {
        load_prior_book_with_metadata(BufReader::new(file)).map_err(std::io::Error::other)
    }) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("error: validating {}: {error}", args.book.display());
            return Ok(ExitCode::from(3));
        }
    };
    let compatibility = match manifest.verify_book_metadata(loaded.metadata.as_ref()) {
        Ok(compatibility) => compatibility,
        Err(error) => {
            eprintln!("error: {error}");
            return Ok(super::exit_code_for_lineprior_error(&error));
        }
    };

    match compatibility {
        PriorArtifactBookCompatibility::Versioned => {
            println!("verified: manifest, input bytes, book bytes, and schema-v1 book metadata");
            Ok(ExitCode::SUCCESS)
        }
        PriorArtifactBookCompatibility::LegacyFingerprint => {
            eprintln!(
                "warning: hashes and legacy config fingerprint verified; complete book provenance is unavailable"
            );
            Ok(ExitCode::from(1))
        }
        PriorArtifactBookCompatibility::Headerless => {
            eprintln!(
                "warning: hashes verified; headerless book has no embedded config provenance"
            );
            Ok(ExitCode::from(1))
        }
        _ => {
            eprintln!("warning: hashes verified; book metadata uses a newer compatibility mode");
            Ok(ExitCode::from(1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_hash_matches_known_sha256() {
        assert_eq!(
            sha256(&b"abc"[..]).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
