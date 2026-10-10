use super::{exit_code_for_lineprior_error, same_path};
use anyhow::{Context, Result};
use clap::Args;
use lineprior::{load_prior_book, load_prior_book_binary, save_prior_book, save_prior_book_binary};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Args)]
pub struct PackArgs {
    input: PathBuf,
    #[arg(long)]
    out: PathBuf,
}
#[derive(Args)]
pub struct UnpackArgs {
    input: PathBuf,
    #[arg(long)]
    out: PathBuf,
}

pub fn pack(args: PackArgs) -> Result<ExitCode> {
    if same_path(&args.input, &args.out) {
        eprintln!("error: input and --out must be different files");
        return Ok(ExitCode::from(3));
    }
    let input = match File::open(&args.input) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("error: opening {}: {err}", args.input.display());
            return Ok(ExitCode::from(3));
        }
    };
    let book = match load_prior_book(input) {
        Ok(book) => book,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(exit_code_for_lineprior_error(&err));
        }
    };
    let out =
        File::create(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
    save_prior_book_binary(&book, BufWriter::new(out)).context("writing compact prior book")?;
    Ok(ExitCode::SUCCESS)
}
pub fn unpack(args: UnpackArgs) -> Result<ExitCode> {
    if same_path(&args.input, &args.out) {
        eprintln!("error: input and --out must be different files");
        return Ok(ExitCode::from(3));
    }
    let input = match File::open(&args.input) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("error: opening {}: {err}", args.input.display());
            return Ok(ExitCode::from(3));
        }
    };
    let book = match load_prior_book_binary(input) {
        Ok(book) => book,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(exit_code_for_lineprior_error(&err));
        }
    };
    let out =
        File::create(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
    let mut writer = BufWriter::new(out);
    save_prior_book(&book, &mut writer).context("writing JSONL prior book")?;
    writer.flush().context("flushing JSONL prior book")?;
    Ok(ExitCode::SUCCESS)
}
