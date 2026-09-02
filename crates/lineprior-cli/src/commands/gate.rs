use anyhow::{Context, Result};
use clap::Args;
use lineprior::{
    GateAcquisition, GateAcquisitionConfig, GateAcquisitionQuery, GateFitReport, GateModel,
    GateModelConfig, GateObservation, GatePrediction, GateQuery, GateVerdictConfig,
    GateVerdictPrediction, MonotonicDirection,
};
use serde::Serialize;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Args)]
pub struct GateArgs {
    /// JSONL gate-history observations.
    input: PathBuf,

    /// Write the JSON report here instead of stdout.
    #[arg(long)]
    out: Option<PathBuf>,

    /// Query feature in `name=value` form; repeat for a candidate prediction.
    #[arg(long = "feature", value_parser = parse_feature, action = clap::ArgAction::Append)]
    features: Vec<(String, f64)>,

    /// Monotonic constraint in `name=increasing|decreasing` form.
    #[arg(long = "monotonic", value_parser = parse_monotonic, action = clap::ArgAction::Append)]
    monotonic_constraints: Vec<(String, MonotonicDirection)>,

    /// Elo delta above which a verdict is PASS.
    #[arg(long, default_value_t = 10.0)]
    pass_threshold: f64,

    /// Elo delta below which a verdict is FAIL.
    #[arg(long, default_value_t = -10.0)]
    fail_threshold: f64,

    /// Incumbent Elo delta for expected-improvement acquisition.
    #[arg(long, default_value_t = 0.0)]
    baseline_elo: f64,

    /// Expected cost of the gate run; enables acquisition output.
    #[arg(long)]
    expected_gate_cost: Option<f64>,
}

#[derive(Serialize)]
struct GateOutput {
    fit: GateFitReport,
    prediction: Option<GatePrediction>,
    verdict: Option<GateVerdictPrediction>,
    acquisition: Option<GateAcquisition>,
}

pub fn run(args: GateArgs) -> Result<ExitCode> {
    let file = match File::open(&args.input) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("error: opening {}: {err}", args.input.display());
            return Ok(ExitCode::from(3));
        }
    };
    let mut observations = Vec::new();
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = line_index + 1;
        let line = line.with_context(|| format!("reading line {line_number}"))?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<GateObservation>(&line) {
            Ok(observation) => observations.push(observation),
            Err(err) => {
                eprintln!("error: line {line_number}: invalid gate JSON: {err}");
                return Ok(ExitCode::from(3));
            }
        }
    }

    let mut config = GateModelConfig::default();
    for (name, direction) in args.monotonic_constraints {
        config.monotonic_constraints.insert(name, direction);
    }
    let fitted = match GateModel::fit(&observations, &config) {
        Ok(output) => output,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(3));
        }
    };

    let (prediction, verdict, acquisition) = if args.features.is_empty() {
        if args.expected_gate_cost.is_some() {
            eprintln!("error: --expected-gate-cost requires at least one --feature");
            return Ok(ExitCode::from(3));
        }
        (None, None, None)
    } else {
        let query = GateQuery {
            features: args.features.into_iter().collect(),
        };
        let verdict = match fitted.model.predict_verdict(
            &query,
            &GateVerdictConfig {
                fail_threshold: args.fail_threshold,
                pass_threshold: args.pass_threshold,
            },
        ) {
            Ok(verdict) => verdict,
            Err(err) => {
                eprintln!("error: {err}");
                return Ok(ExitCode::from(3));
            }
        };
        let prediction = verdict.prediction.clone();
        let acquisition = match args.expected_gate_cost {
            Some(expected_gate_cost) => match fitted.model.acquire(
                &GateAcquisitionQuery {
                    query,
                    expected_gate_cost,
                },
                &GateAcquisitionConfig {
                    baseline_elo: args.baseline_elo,
                },
            ) {
                Ok(acquisition) => Some(acquisition),
                Err(err) => {
                    eprintln!("error: {err}");
                    return Ok(ExitCode::from(3));
                }
            },
            None => None,
        };
        (Some(prediction), Some(verdict), acquisition)
    };

    let output = GateOutput {
        fit: fitted.report,
        prediction,
        verdict,
        acquisition,
    };
    let json = serde_json::to_string_pretty(&output).context("serializing gate report")?;
    match args.out {
        Some(path) => {
            let file =
                File::create(&path).with_context(|| format!("creating {}", path.display()))?;
            use std::io::Write;
            let mut writer = BufWriter::new(file);
            writer.write_all(json.as_bytes())?;
            writer.write_all(b"\n")?;
        }
        None => println!("{json}"),
    }
    Ok(ExitCode::from(0))
}

fn parse_feature(value: &str) -> Result<(String, f64), String> {
    let (name, raw) = value
        .split_once('=')
        .ok_or_else(|| "feature must use name=value".to_string())?;
    if name.is_empty() {
        return Err("feature name must not be empty".to_string());
    }
    let number = raw
        .parse::<f64>()
        .map_err(|_| format!("feature value is not a number: {raw}"))?;
    if !number.is_finite() {
        return Err("feature value must be finite".to_string());
    }
    Ok((name.to_string(), number))
}

fn parse_monotonic(value: &str) -> Result<(String, MonotonicDirection), String> {
    let (name, direction) = value
        .split_once('=')
        .ok_or_else(|| "monotonic must use name=increasing|decreasing".to_string())?;
    if name.is_empty() {
        return Err("monotonic feature name must not be empty".to_string());
    }
    let direction = match direction {
        "increasing" => MonotonicDirection::Increasing,
        "decreasing" => MonotonicDirection::Decreasing,
        _ => return Err("monotonic direction must be increasing or decreasing".to_string()),
    };
    Ok((name.to_string(), direction))
}
