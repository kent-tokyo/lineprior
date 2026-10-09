use anyhow::Result;
use clap::{Args, ValueEnum};
use lineprior::{
    GateObservation, OffPolicyObservation, parse_jsonl, validate_gate_observations,
    validate_offpolicy_observations,
};
use serde::de::DeserializeOwned;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum ValidateKind {
    /// Historical state/action observations consumed by build/eval.
    #[default]
    #[value(name = "observations")]
    Observations,
    /// Logged decisions consumed by the offpolicy command.
    Offpolicy,
    /// Historical gate outcomes consumed by the gate command.
    Gate,
}

#[derive(Args)]
pub struct ValidateArgs {
    /// Input JSONL observation log to validate.
    input: PathBuf,

    /// Fail on the first invalid record instead of collecting warnings.
    #[arg(long)]
    strict: bool,

    /// Select the JSONL row contract to validate.
    #[arg(long, value_enum, default_value_t)]
    kind: ValidateKind,
}

pub fn run(args: ValidateArgs) -> Result<ExitCode> {
    let file = match File::open(&args.input) {
        Ok(f) => f,
        Err(err) => {
            eprintln!("error: opening {}: {err}", args.input.display());
            return Ok(ExitCode::from(3));
        }
    };

    match args.kind {
        ValidateKind::Observations => validate_observations(file, args.strict),
        ValidateKind::Offpolicy => validate_offpolicy(file),
        ValidateKind::Gate => validate_gate(file),
    }
}

fn validate_observations(file: File, strict: bool) -> Result<ExitCode> {
    let parsed = match parse_jsonl(file, strict) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(super::exit_code_for_lineprior_error(&err));
        }
    };

    for warning in &parsed.warnings {
        eprintln!("warning: {warning}");
    }
    println!(
        "{} valid observation(s), {} warning(s)",
        parsed.observations.len(),
        parsed.warnings.len()
    );

    if parsed.observations.is_empty() {
        eprintln!("error: no usable data");
        return Ok(ExitCode::from(2));
    }
    if !parsed.warnings.is_empty() {
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::from(0))
}

fn validate_offpolicy(file: File) -> Result<ExitCode> {
    let observations = match parse_typed_jsonl::<OffPolicyObservation>(file, "off-policy") {
        Ok(observations) => observations,
        Err(message) => {
            eprintln!("error: {message}");
            return Ok(ExitCode::from(3));
        }
    };
    if observations.is_empty() {
        eprintln!("error: no usable data");
        return Ok(ExitCode::from(2));
    }
    if let Err(err) = validate_offpolicy_observations(&observations) {
        eprintln!("error: {err}");
        return Ok(ExitCode::from(3));
    }
    println!("{} valid off-policy observation(s)", observations.len());
    Ok(ExitCode::from(0))
}

fn validate_gate(file: File) -> Result<ExitCode> {
    let observations = match parse_typed_jsonl::<GateObservation>(file, "gate") {
        Ok(observations) => observations,
        Err(message) => {
            eprintln!("error: {message}");
            return Ok(ExitCode::from(3));
        }
    };
    if observations.is_empty() {
        eprintln!("error: no usable data");
        return Ok(ExitCode::from(2));
    }
    if let Err(err) = validate_gate_observations(&observations) {
        eprintln!("error: {err}");
        return Ok(ExitCode::from(3));
    }
    println!("{} valid gate observation(s)", observations.len());
    Ok(ExitCode::from(0))
}

fn parse_typed_jsonl<T: DeserializeOwned>(
    file: File,
    label: &str,
) -> std::result::Result<Vec<T>, String> {
    let mut rows = Vec::new();
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = line_index + 1;
        let line = line.map_err(|err| format!("reading line {line_number}: {err}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let row = serde_json::from_str(&line)
            .map_err(|err| format!("line {line_number}: invalid {label} JSON: {err}"))?;
        rows.push(row);
    }
    Ok(rows)
}
