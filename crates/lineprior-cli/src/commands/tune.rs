use super::{ConfidenceModeArg, ObjectiveArg, SplitBy};
use anyhow::{Context, Result};
use clap::{Args, ValueEnum};
use lineprior::{
    BuildConfig, EvalConfig, TuneConstraints, TuneObjective, TuneOutput, TuneParam,
    build_candidate_result, evaluate, expand_grid,
};
use std::fs::File;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Args)]
pub struct TuneArgs {
    /// Input JSONL observation log.
    input: PathBuf,

    /// How to assign observations to train/test (same as `eval`, and
    /// reused identically -- unswept -- across every candidate).
    #[arg(long, value_enum, default_value_t = SplitBy::Sequence)]
    split_by: SplitBy,

    /// Fraction of sequences assigned to the train split, shared by every candidate.
    #[arg(long, default_value_t = 0.8)]
    train_ratio: f64,

    /// One BuildConfig knob to sweep, as `key=value1,value2,...`. Repeatable
    /// (one `--param` per key). Supported keys: confidence-mode,
    /// min-confidence, smoothing-alpha, confidence-k, confidence-z,
    /// min-count, min-weighted-count, draw-value, time-decay-half-life-days
    /// (accepts `none`), default-source-weight. A key not swept stays at
    /// its `BuildConfig::default()` value for every candidate.
    #[arg(long = "param")]
    params: Vec<String>,

    /// Which held-out metric to rank candidates by.
    #[arg(long, value_enum, default_value_t = ObjectiveArg::CoveredMrr)]
    objective: ObjectiveArg,

    /// Reject a candidate below this observation-weighted covered fraction
    /// (1 - fallback_rate). Required when --objective is top1-at-min-coverage.
    #[arg(long)]
    min_covered_fraction: Option<f64>,

    /// Reject a candidate above this fallback rate.
    #[arg(long)]
    max_fallback_rate: Option<f64>,

    /// Reject a candidate below this top1_hit_rate.
    #[arg(long)]
    min_top1_hit_rate: Option<f64>,

    /// "Now", as a Unix timestamp, applied to every candidate (swept or
    /// not). Required whenever a swept time-decay-half-life-days value is
    /// not `none` -- same no-implicit-wall-clock rule as `build`/`eval`.
    #[arg(long)]
    time_decay_reference_unix_seconds: Option<i64>,

    /// Write the JSON report here instead of stdout.
    #[arg(long)]
    out: Option<PathBuf>,

    /// Save the winning candidate's BuildConfig here, loadable later via
    /// `lineprior build`/`eval --config`.
    #[arg(long)]
    save_best_config: Option<PathBuf>,

    /// Fail on the first invalid record instead of skipping it with a warning.
    #[arg(long)]
    strict: bool,
}

/// Parses one `--param key=v1,v2,...` occurrence. Pure and clap-free so
/// it's unit-testable without spawning the binary.
fn parse_param(raw: &str) -> Result<TuneParam> {
    let (key, values_str) = raw.split_once('=').ok_or_else(|| {
        anyhow::anyhow!("invalid --param {raw:?}, expected key=value1,value2,...")
    })?;
    let raw_values: Vec<&str> = values_str.split(',').collect();
    if raw_values.iter().any(|v| v.is_empty()) {
        anyhow::bail!("invalid --param {raw:?}: empty value in list");
    }

    match key {
        "confidence-mode" => {
            let values = raw_values
                .iter()
                .map(|v| {
                    ConfidenceModeArg::from_str(v, true)
                        .map(lineprior::ConfidenceMode::from)
                        .map_err(|_| anyhow::anyhow!("invalid confidence-mode value {v:?}"))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(TuneParam::ConfidenceMode(values))
        }
        "min-confidence" => Ok(TuneParam::MinConfidence(parse_floats(&raw_values)?)),
        "smoothing-alpha" => Ok(TuneParam::SmoothingAlpha(parse_floats(&raw_values)?)),
        "confidence-k" => Ok(TuneParam::ConfidenceK(parse_floats(&raw_values)?)),
        "confidence-z" => Ok(TuneParam::ConfidenceZ(parse_floats(&raw_values)?)),
        "min-count" => Ok(TuneParam::MinCount(parse_uints(&raw_values)?)),
        "min-weighted-count" => Ok(TuneParam::MinWeightedCount(parse_floats(&raw_values)?)),
        "draw-value" => Ok(TuneParam::DrawValue(parse_floats(&raw_values)?)),
        "time-decay-half-life-days" => {
            let values = raw_values
                .iter()
                .map(|v| {
                    if *v == "none" {
                        Ok(None)
                    } else {
                        v.parse::<f64>().map(Some).map_err(|_| {
                            anyhow::anyhow!("invalid time-decay-half-life-days value {v:?}")
                        })
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(TuneParam::TimeDecayHalfLifeDays(values))
        }
        "default-source-weight" => Ok(TuneParam::DefaultSourceWeight(parse_floats(&raw_values)?)),
        other => anyhow::bail!(
            "unknown --param key {other:?}; supported: confidence-mode, min-confidence, \
             smoothing-alpha, confidence-k, confidence-z, min-count, min-weighted-count, \
             draw-value, time-decay-half-life-days, default-source-weight"
        ),
    }
}

fn parse_floats(raw_values: &[&str]) -> Result<Vec<f64>> {
    raw_values
        .iter()
        .map(|v| {
            v.parse::<f64>()
                .map_err(|_| anyhow::anyhow!("invalid numeric value {v:?}"))
        })
        .collect()
}

fn parse_uints(raw_values: &[&str]) -> Result<Vec<u64>> {
    raw_values
        .iter()
        .map(|v| {
            v.parse::<u64>()
                .map_err(|_| anyhow::anyhow!("invalid integer value {v:?}"))
        })
        .collect()
}

pub fn run(args: TuneArgs) -> Result<ExitCode> {
    let SplitBy::Sequence = args.split_by;

    if matches!(args.objective, ObjectiveArg::Top1AtMinCoverage)
        && args.min_covered_fraction.is_none()
    {
        eprintln!("error: --objective top1-at-min-coverage requires --min-covered-fraction");
        return Ok(ExitCode::from(3));
    }

    let params: Vec<TuneParam> = match args.params.iter().map(|p| parse_param(p)).collect() {
        Ok(params) => params,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(3));
        }
    };

    let sweeps_enabled_decay = params.iter().any(|p| {
        matches!(p, TuneParam::TimeDecayHalfLifeDays(values) if values.iter().any(|v| v.is_some()))
    });
    if sweeps_enabled_decay && args.time_decay_reference_unix_seconds.is_none() {
        eprintln!(
            "error: --time-decay-reference-unix-seconds is required when a swept \
             time-decay-half-life-days value is not `none`"
        );
        return Ok(ExitCode::from(3));
    }

    let candidates = expand_grid(&BuildConfig::default(), &params);
    let eval_config = EvalConfig {
        train_ratio: args.train_ratio,
        top_k: vec![1],
        calibration_bins: None,
        thresholds: Vec::new(),
    };
    let constraints = TuneConstraints {
        min_covered_fraction: args.min_covered_fraction,
        max_fallback_rate: args.max_fallback_rate,
        min_top1_hit_rate: args.min_top1_hit_rate,
    };
    let objective: TuneObjective = args.objective.into();

    // ponytail: reopens the input file twice per candidate (matching
    // eval.rs's own open-twice-per-call shape, just in a loop) rather than
    // parsing once and replaying an in-memory Vec<Observation> -- keeps the
    // streaming/bounded-memory property but costs O(candidates) re-parses.
    // Upgrade path if a grid gets large: parse once, replay from memory.
    let mut all_results = Vec::new();
    let mut warnings = Vec::new();
    let mut skipped_config_count = 0usize;

    for (config_id, mut config) in candidates {
        if let Some(reference) = args.time_decay_reference_unix_seconds {
            config.time_decay_reference_unix_seconds = Some(reference);
        }

        let train_file = match File::open(&args.input) {
            Ok(f) => f,
            Err(err) => {
                eprintln!("error: opening {}: {err}", args.input.display());
                return Ok(ExitCode::from(3));
            }
        };
        let test_file = match File::open(&args.input) {
            Ok(f) => f,
            Err(err) => {
                eprintln!("error: opening {}: {err}", args.input.display());
                return Ok(ExitCode::from(3));
            }
        };

        match evaluate(train_file, test_file, args.strict, &config, &eval_config) {
            Ok(output) => {
                let result = build_candidate_result(
                    config_id,
                    config,
                    &output.report,
                    objective,
                    &constraints,
                );
                all_results.push(result);
            }
            Err(err) => {
                skipped_config_count += 1;
                warnings.push(format!("{config_id}: {err}"));
            }
        }
    }

    let tune_output = TuneOutput::from_results(
        all_results,
        objective,
        constraints,
        skipped_config_count,
        warnings,
    );

    if let Some(path) = &args.save_best_config {
        match &tune_output.best {
            Some(best) => {
                let json = serde_json::to_string_pretty(&best.build_config)?;
                std::fs::write(path, json)
                    .with_context(|| format!("writing {}", path.display()))?;
            }
            None => {
                eprintln!("error: no candidate config satisfied the constraints; nothing to save");
                return Ok(ExitCode::from(2));
            }
        }
    }

    let json = serde_json::to_string_pretty(&tune_output)?;
    match args.out {
        Some(path) => {
            std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
        }
        None => println!("{json}"),
    }

    if tune_output.evaluated_config_count == 0 {
        eprintln!("error: no usable data (every candidate config failed to evaluate)");
        return Ok(ExitCode::from(2));
    }
    if !tune_output.warnings.is_empty() || tune_output.skipped_config_count > 0 {
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::from(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_confidence_mode_values() {
        let param = parse_param("confidence-mode=heuristic,wilson-lower-bound,hybrid").unwrap();
        assert!(matches!(
            param,
            TuneParam::ConfidenceMode(values) if values == vec![
                lineprior::ConfidenceMode::Heuristic,
                lineprior::ConfidenceMode::WilsonLowerBound,
                lineprior::ConfidenceMode::Hybrid,
            ]
        ));
    }

    #[test]
    fn parses_numeric_and_uint_params() {
        assert!(matches!(
            parse_param("min-confidence=0.0,0.5").unwrap(),
            TuneParam::MinConfidence(values) if values == vec![0.0, 0.5]
        ));
        assert!(matches!(
            parse_param("min-count=1,3,5").unwrap(),
            TuneParam::MinCount(values) if values == vec![1, 3, 5]
        ));
    }

    #[test]
    fn none_is_accepted_only_for_time_decay_half_life_days() {
        assert!(matches!(
            parse_param("time-decay-half-life-days=none,30,90").unwrap(),
            TuneParam::TimeDecayHalfLifeDays(values) if values == vec![None, Some(30.0), Some(90.0)]
        ));
        // "none" isn't a valid f64 for any other key.
        assert!(parse_param("min-confidence=none").is_err());
    }

    #[test]
    fn unknown_param_key_is_rejected() {
        let err = parse_param("not-a-real-key=1,2").unwrap_err();
        assert!(err.to_string().contains("unknown --param key"));
    }

    #[test]
    fn invalid_numeric_value_is_rejected() {
        assert!(parse_param("min-confidence=oops").is_err());
        assert!(parse_param("min-count=1.5").is_err()); // not a valid u64
    }

    #[test]
    fn missing_equals_sign_is_rejected() {
        assert!(parse_param("min-confidence").is_err());
    }
}
