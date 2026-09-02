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
    let book = load_prior_book(
        File::open(&args.input).with_context(|| format!("opening {}", args.input.display()))?,
    )?;
    let out =
        File::create(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
    save_prior_book_binary(&book, BufWriter::new(out))?;
    Ok(ExitCode::SUCCESS)
}
pub fn unpack(args: UnpackArgs) -> Result<ExitCode> {
    let book = load_prior_book_binary(
        File::open(&args.input).with_context(|| format!("opening {}", args.input.display()))?,
    )?;
    let out =
        File::create(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
    let mut writer = BufWriter::new(out);
    save_prior_book(&book, &mut writer)?;
    writer.flush()?;
    Ok(ExitCode::SUCCESS)
}
