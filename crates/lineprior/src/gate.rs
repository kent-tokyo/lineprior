//! Gate outcome prediction: a small, regularized surrogate that predicts a
//! training candidate's real-gate Elo delta (and how much to trust that
//! prediction) from cheap validation-time diagnostics, so an expensive gate
//! run (hundreds of real games) can be reserved for candidates worth it.
//!
//! Deliberately separate from the `(state, action)` prior model in the rest
//! of this crate: the input here is a caller-named feature vector, not an
//! opaque state/action pair, and the output is a point estimate with an
//! uncertainty interval, not a ranked candidate list. Kept in this crate for
//! now (see the module's own positioning note in the project's task log) --
//! split into its own crate if this grows a CLI surface or its own
//! dependencies.
//!
//! Round A scope only: fit, predict, and a calibration report. No
//! acquisition function, no CLI, no monotonic constraints, no bootstrap --
//! see the project task log for why each is deferred.

use crate::error::{Error, Result};
use crate::hash::fnv1a;
use crate::model::DEFAULT_CONFIDENCE_Z;
use crate::score::effective_sample_size;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One historical training candidate that has already been through a real
/// gate run, used as training data for [`GateModel::fit`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateObservation {
    pub candidate_id: String,
    /// Opaque, caller-composed grouping key (e.g. `experiment_family + "|" +
    /// recipe + "|" + lineage + "|" + dataset_version`) used for group-aware
    /// cross-validation so a leaked axis of relatedness never splits across
    /// train/validation. Never parsed or decomposed by this crate.
    pub group_id: String,
    /// Named validation-time diagnostics (e.g. `valid_cp_mse_delta`,
    /// `output_std`, `conflict_rate`). Every observation passed to one
    /// [`GateModel::fit`] call must carry exactly the same set of feature
    /// names (see [`Error::InconsistentGateFeatures`]).
    ///
    /// Deliberately excludes anything like a `training_seed`: a seed is a
    /// categorical id, not a quantity where "more" or "less" means anything
    /// to a linear model, and seed/shuffle stability is a different
    /// project's concern, not this one's.
    pub features: BTreeMap<String, f64>,
    /// Measured Elo delta from the real gate run -- the regression label.
    pub gate_elo_delta: f64,
    /// Games played behind `gate_elo_delta`, used as this label's
    /// reliability weight in the ridge fit: a 20-game delta is noisier than
    /// a 396-game one and should count for less. Must be `> 0.0`.
    pub gate_games_played: f64,
}

/// A not-yet-gated candidate to score with [`GateModel::predict`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateQuery {
    pub features: BTreeMap<String, f64>,
}

/// [`GateModel::predict`]'s output for one [`GateQuery`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GatePrediction {
    pub expected_elo: f64,
    /// `expected_elo -/+ GateModelConfig::interval_z` standard deviations of
    /// the model's *latent-mean* uncertainty -- how much to trust
    /// `expected_elo` as an estimate of this candidate's true strength, not
    /// the added noise of any one future gate run (see the module doc and
    /// [`predictive_variance`]).
    pub interval_low: f64,
    pub interval_high: f64,
    /// `P(true Elo delta > 0)` under the model's Gaussian posterior.
    pub probability_positive: f64,
    /// Feature names the fitted model expects that this query didn't
    /// provide. Each missing feature is scored as its training-set mean
    /// (standardized value `0.0`) -- the same "absent data shrinks toward
    /// the population, it isn't refused" convention `shrink_toward` already
    /// uses elsewhere in this crate -- and reported here rather than
    /// silently assumed, so the caller can decide whether to trust the
    /// result. Empty in the common case.
    pub missing_features: Vec<String>,
    /// Feature names this query provided that the fitted model never
    /// trained on -- ignored (there is no coefficient for them), reported
    /// here rather than silently dropped.
    pub unknown_features: Vec<String>,
}

/// Tuning knobs for [`GateModel::fit`].
#[derive(Debug, Clone)]
pub struct GateModelConfig {
    /// L2 strength candidates, grid-searched via group-aware cross-validation
    /// (held-out weighted RMSE, ties broken by earliest-in-list). `0.0` is
    /// deliberately excluded from the default grid: in the small-n,
    /// p-approx-n regime this module targets, letting OLS (no
    /// regularization) into the grid tends to win a fold by chance and pick
    /// an unstable model.
    pub lambda_grid: Vec<f64>,
    /// Requested number of cross-validation folds, used at *every* level of
    /// [`GateModel::fit`]'s nested CV (the plain deployed-lambda selection,
    /// the outer folds, and each outer fold's own inner folds). Falls back
    /// to leave-one-group-out when fewer than `cv_folds` distinct
    /// `group_id`s exist at that level (see [`GateModel::fit`]'s doc
    /// comment).
    pub cv_folds: usize,
    /// Number of equal-width bins over `[0, 1]` for the calibration report.
    pub calibration_bins: usize,
    /// z-score for `GatePrediction`'s interval width. Reuses
    /// [`DEFAULT_CONFIDENCE_Z`], the same one-sided-conservative z already
    /// used for this crate's Wilson-bound confidence.
    pub interval_z: f64,
}

/// `1e-4` through `100.0`, log-spaced-ish, chosen to bias toward strong
/// regularization for a p-approx-n regime -- see [`GateModelConfig::lambda_grid`].
pub fn default_gate_lambda_grid() -> Vec<f64> {
    vec![
        1e-4, 3e-4, 1e-3, 3e-3, 1e-2, 3e-2, 1e-1, 3e-1, 1.0, 3.0, 10.0, 30.0, 100.0,
    ]
}

impl Default for GateModelConfig {
    fn default() -> Self {
        Self {
            lambda_grid: default_gate_lambda_grid(),
            cv_folds: 5,
            calibration_bins: 5,
            interval_z: DEFAULT_CONFIDENCE_Z,
        }
    }
}

/// Ranking quality for one equal-width `probability_positive` bin, over
/// out-of-fold predictions collected during [`GateModel::fit`]'s *nested*
/// cross-validation pass (see its doc comment) -- not the single-level CV
/// used to pick the deployed model's lambda, which would be optimistically
/// biased if reused here directly. Mirrors `eval.rs`'s `CalibrationBin`
/// shape: a well-calibrated model should show `empirical_positive_rate`
/// tracking bin probability roughly 1:1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GateCalibrationBin {
    pub min_probability: f64,
    pub max_probability: f64,
    pub num_observations: u64,
    /// Fraction of this bin's observations whose actual `gate_elo_delta` was
    /// `> 0.0`. `None` when the bin saw no observations.
    pub empirical_positive_rate: Option<f64>,
}

/// Diagnostics from a [`GateModel::fit`] call, for deciding whether the
/// fitted model is trustworthy enough to act on before any acquisition
/// logic is layered on top (deferred to a later round).
#[derive(Debug, Clone, Serialize)]
pub struct GateFitReport {
    pub selected_lambda: f64,
    /// Number of outer folds the nested cross-validation pass actually
    /// evaluated (non-empty on both the training and validation side) --
    /// measured from [`nested_cross_validate`]'s own fold loop, not assumed
    /// from the requested `min(GateModelConfig::cv_folds, num_groups)`. The
    /// two agree whenever `num_groups >= cv_folds`, since balanced
    /// GroupKFold (see [`assign_folds`]) guarantees every requested fold is
    /// non-empty in that case.
    pub cv_folds_used: usize,
    pub num_observations: usize,
    pub num_groups: usize,
    /// Held-out weighted RMSE from *nested* group cross-validation (see
    /// [`GateModel::fit`]'s doc comment) -- an honest estimate of how this
    /// kind of model performs on an unseen group, not tied to
    /// `selected_lambda` specifically (each outer fold may pick a different
    /// lambda via its own inner CV). From the same nested-CV pass
    /// `calibration` is built from.
    pub weighted_rmse: f64,
    pub calibration: Vec<GateCalibrationBin>,
}

/// Result of [`GateModel::fit`]: the fitted model plus its own diagnostics.
#[derive(Debug, Clone)]
pub struct GateFitOutput {
    pub model: GateModel,
    pub report: GateFitReport,
}

/// One nested-CV out-of-fold audit row: `predicted_elo`/`probability_positive`
/// were produced by a model that never saw this candidate's own row *or* its
/// `group_id` during fitting *or* lambda selection (see
/// [`GateModel::fit_with_validation`]'s doc comment) -- exactly the
/// population `GateFitReport::weighted_rmse`/`calibration` are computed
/// from, exposed per-candidate for real-data validation (comparing against
/// an external baseline, auditing calibration and interval coverage by
/// hand) before building anything on top of Round A. Not deduplicated by
/// `candidate_id`: a caller whose data happens to repeat one is still owed
/// every row, not a silently-collapsed one.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GateOofPrediction {
    pub candidate_id: String,
    pub group_id: String,
    pub actual_elo: f64,
    pub predicted_elo: f64,
    pub prediction_stddev: f64,
    /// `predicted_elo -/+ GateModelConfig::interval_z` standard deviations --
    /// same latent-candidate-strength interval [`GatePrediction::interval_low`]
    /// documents, not a future single-gate-result prediction interval. See
    /// [`GateValidationOutput::interval_level`] for this interval's
    /// confidence level, stated once rather than repeated per row.
    pub interval_low: f64,
    pub interval_high: f64,
    pub probability_positive: f64,
    /// `actual_elo - predicted_elo`.
    pub residual: f64,
    pub gate_games_played: f64,
    /// Which outer CV fold held this row out (see
    /// [`GateModel::fit_with_validation`]'s doc comment). Rows sharing an
    /// `outer_fold` were scored by the identical fitted model -- and, under
    /// the leave-one-group-out fallback, share the identical `group_id` too.
    pub outer_fold: usize,
    /// The lambda that fold's own inner CV selected (or
    /// [`most_conservative_lambda`], when that fold's training rows had too
    /// few distinct groups to run one) -- not necessarily
    /// `GateFitReport::selected_lambda`, which is chosen separately over the
    /// whole dataset for the deployed model.
    pub inner_selected_lambda: f64,
}

/// Result of [`GateModel::fit_with_validation`]: everything [`GateFitOutput`]
/// carries, plus the nested-CV out-of-fold audit table and the confidence
/// level its intervals represent. A superset, not a fork -- `report` is the
/// exact same [`GateFitReport`] `fit` returns, so the two functions can
/// never disagree about the aggregate metrics.
#[derive(Debug, Clone)]
pub struct GateValidationOutput {
    pub model: GateModel,
    pub report: GateFitReport,
    pub oof_predictions: Vec<GateOofPrediction>,
    /// `2 * Phi(GateModelConfig::interval_z) - 1`, e.g. `~0.95` for the
    /// default `interval_z` (`DEFAULT_CONFIDENCE_Z` = `1.96`) -- the
    /// two-sided confidence level `interval_low`/`interval_high` (on both
    /// [`GatePrediction`] and [`GateOofPrediction`]) represent, stated once
    /// here rather than repeated on every row.
    pub interval_level: f64,
}

/// A fitted gate-outcome surrogate. Opaque by design -- [`GateModel::fit`]
/// and [`GateModel::predict`] are the API; the standardized-space
/// coefficients are an implementation detail, not something callers should
/// need to read directly.
#[derive(Debug, Clone)]
pub struct GateModel {
    feature_names: Vec<String>,
    feature_mean: Vec<f64>,
    feature_std: Vec<f64>,
    intercept: f64,
    coefficients: Vec<f64>,
    /// `(XᵀWX + lambda*I)⁻¹` in standardized feature space, refit on 100% of
    /// the gated data at the selected lambda -- reused directly for
    /// [`predictive_variance`] so point estimate and uncertainty can never
    /// drift apart.
    m: Vec<Vec<f64>>,
    sigma2: f64,
    /// `1.0 / n_eff` from the same full-data refit `m`/`sigma2` came from --
    /// see [`RidgeFit::intercept_variance_factor`].
    intercept_variance_factor: f64,
    interval_z: f64,
}

impl GateModel {
    /// Fits a weighted ridge regression from `observations` to
    /// `gate_elo_delta`. A thin wrapper over [`Self::fit_with_validation`]
    /// that discards the out-of-fold audit table and interval level (the
    /// table is still computed either way -- this only spares the caller
    /// from receiving/reading it) -- mirrors this crate's existing
    /// `finish`/`finish_with_stats` shape (`build.rs`'s `PriorAccumulator`),
    /// so both entry points share one fitting path and can never disagree
    /// on the model or its metrics.
    pub fn fit(
        observations: &[GateObservation],
        config: &GateModelConfig,
    ) -> Result<GateFitOutput> {
        let validated = Self::fit_with_validation(observations, config)?;
        Ok(GateFitOutput {
            model: validated.model,
            report: validated.report,
        })
    }

    /// Like [`Self::fit`], but also returns the nested-CV out-of-fold audit
    /// table (one [`GateOofPrediction`] per outer-validation row) and the
    /// interval's confidence level -- for validating Round A itself against
    /// real gate history (an external baseline comparison, a calibration
    /// audit, interval-coverage checks) before any acquisition logic is
    /// built on top of it.
    ///
    /// Two group-aware cross-validations run side by side, deliberately not
    /// sharing predictions:
    ///
    /// 1. **Deployed-model lambda** (`report.selected_lambda`): a plain,
    ///    single-level group CV over the *entire* dataset ([`select_lambda`]).
    /// 2. **`report.weighted_rmse`/`report.calibration`/`oof_predictions`**:
    ///    a *nested* group CV ([`nested_cross_validate`]) -- for each outer
    ///    fold, lambda is chosen by an inner CV scoped to that fold's own
    ///    training rows only, then that fold's held-out rows are scored by
    ///    a model fit at the inner-selected lambda. Reusing (1)'s held-out
    ///    predictions directly for the report would be optimistically
    ///    biased: picking the best lambda by its held-out score already
    ///    uses information from every validation fold, so those same
    ///    folds' predictions aren't honestly out-of-sample with respect to
    ///    the selection that produced them. Nesting keeps every reported
    ///    prediction genuinely unseen by both the coefficients *and* the
    ///    lambda that produced them. `oof_predictions` is built from this
    ///    *same* nested-CV pass, not a second run of it -- one population of
    ///    predictions backs both the aggregate metrics and the per-row
    ///    audit table, so they can never describe different data.
    ///
    /// Fold assignment (both levels) is by `group_id`, never by individual
    /// observation, so no axis of relatedness the caller encoded into
    /// `group_id` can leak between train and validation (same rationale as
    /// `eval.rs`'s `sequence_id`-based split). Uses deterministic balanced
    /// GroupKFold ([`assign_folds`]): `min(cv_folds, num_groups)` folds,
    /// each group placed on whichever fold currently holds the least total
    /// `gate_games_played` -- every fold is guaranteed non-empty whenever
    /// `num_groups >= cv_folds` (see `assign_folds`'s doc comment for the
    /// guarantee and its deterministic tie-breaking). Feature
    /// standardization and the ridge fit for a given fold use that fold's
    /// *training* rows only -- the held-out rows are never involved in
    /// choosing their own scale or fit, at either CV level.
    ///
    /// The final returned model is refit on all of `observations` at (1)'s
    /// deployed-model lambda (same "CV picks the config, then refit on 100%
    /// of the data" shape as this crate's `tune` -> `build --config` flow).
    pub fn fit_with_validation(
        observations: &[GateObservation],
        config: &GateModelConfig,
    ) -> Result<GateValidationOutput> {
        validate_config(config)?;
        let feature_names = validate_observations(observations)?;

        let num_features = feature_names.len();
        let required = (num_features + 2).max(6);
        if observations.len() < required {
            return Err(Error::InsufficientGateObservations {
                num_observations: observations.len(),
                num_features,
                required,
            });
        }

        let all_rows: Vec<&GateObservation> = observations.iter().collect();
        let num_groups = distinct_sorted(all_rows.iter().map(|o| o.group_id.as_str())).len();
        if num_groups < 2 {
            return Err(Error::InsufficientGateGroups { num_groups });
        }

        let (fold_ids, effective_folds) = assign_folds(&all_rows, config.cv_folds);

        let (selected_lambda, _plain_rmse, _plain_predictions) = select_lambda(
            &all_rows,
            &feature_names,
            &fold_ids,
            effective_folds,
            &config.lambda_grid,
        );

        let (folds_used, weighted_rmse, fold_predictions) = nested_cross_validate(
            &all_rows,
            &feature_names,
            &fold_ids,
            effective_folds,
            &config.lambda_grid,
            config.cv_folds,
        );
        let calibration = build_calibration(&fold_predictions, config.calibration_bins);
        let oof_predictions = build_oof_table(&fold_predictions, config.interval_z);
        let interval_level = 2.0 * standard_normal_cdf(config.interval_z) - 1.0;

        let weights: Vec<f64> = observations.iter().map(|o| o.gate_games_played).collect();
        let standardizer = Standardizer::fit(&all_rows, &feature_names, &weights);
        let x: Vec<Vec<f64>> = observations
            .iter()
            .map(|o| standardizer.transform(&o.features, &feature_names).0)
            .collect();
        let y: Vec<f64> = observations.iter().map(|o| o.gate_elo_delta).collect();
        let fit = fit_weighted_ridge(&x, &y, &weights, selected_lambda);

        let model = GateModel {
            feature_names,
            feature_mean: standardizer.mean,
            feature_std: standardizer.std,
            intercept: fit.intercept,
            coefficients: fit.coefficients,
            m: fit.m,
            sigma2: fit.sigma2,
            intercept_variance_factor: fit.intercept_variance_factor,
            interval_z: config.interval_z,
        };
        let report = GateFitReport {
            selected_lambda,
            cv_folds_used: folds_used,
            num_observations: observations.len(),
            num_groups,
            weighted_rmse,
            calibration,
        };
        Ok(GateValidationOutput {
            model,
            report,
            oof_predictions,
            interval_level,
        })
    }

    /// Scores `query` against the fitted model. Infallible: an unseen
    /// feature name in either direction is reported via
    /// `GatePrediction::missing_features`/`unknown_features` rather than
    /// rejected, mirroring [`crate::model::PriorBook::query`]'s "unseen input
    /// is a fallback, not an error" convention.
    pub fn predict(&self, query: &GateQuery) -> GatePrediction {
        let (x, missing_features, unknown_features) = transform(
            &query.features,
            &self.feature_names,
            &self.feature_mean,
            &self.feature_std,
        );

        let expected_elo = self.intercept + dot(&x, &self.coefficients);
        let variance =
            predictive_variance(&self.m, &x, self.sigma2, self.intercept_variance_factor);
        let sd = variance.sqrt();
        let probability_positive = probability_positive(expected_elo, variance, sd);

        GatePrediction {
            expected_elo,
            interval_low: expected_elo - self.interval_z * sd,
            interval_high: expected_elo + self.interval_z * sd,
            probability_positive,
            missing_features,
            unknown_features,
        }
    }
}

/// `x.is_finite() && x > 0.0`, factored out so the validation checks below
/// never negate a `>` comparison directly (that reads fine for plain floats
/// but is a clippy footgun in general since NaN breaks De Morgan's laws for
/// partially-ordered types).
fn is_positive_finite(x: f64) -> bool {
    x.is_finite() && x > 0.0
}

fn validate_config(config: &GateModelConfig) -> Result<()> {
    if config.lambda_grid.is_empty() {
        return Err(Error::InvalidConfig {
            message: "gate lambda_grid must not be empty".to_string(),
        });
    }
    if config.lambda_grid.iter().any(|&l| !is_positive_finite(l)) {
        return Err(Error::InvalidConfig {
            message: "gate lambda_grid values must be finite and > 0.0".to_string(),
        });
    }
    if config.cv_folds < 2 {
        return Err(Error::InvalidConfig {
            message: "gate cv_folds must be >= 2".to_string(),
        });
    }
    if config.calibration_bins == 0 {
        return Err(Error::InvalidConfig {
            message: "gate calibration_bins must be >= 1".to_string(),
        });
    }
    if !is_positive_finite(config.interval_z) {
        return Err(Error::InvalidConfig {
            message: "gate interval_z must be finite and > 0.0".to_string(),
        });
    }
    Ok(())
}

/// Validates every observation (finite feature/label/weight values, positive
/// weight, an identical feature-name set across all rows) and returns that
/// shared, sorted feature-name list. Training-time inconsistency is a hard
/// error (unlike `predict`'s query-time handling) -- see
/// [`Error::InconsistentGateFeatures`]'s doc comment for why.
fn validate_observations(observations: &[GateObservation]) -> Result<Vec<String>> {
    if observations.is_empty() {
        return Err(Error::InsufficientGateObservations {
            num_observations: 0,
            num_features: 0,
            required: 6,
        });
    }
    let feature_names: Vec<String> = observations[0].features.keys().cloned().collect();
    for obs in observations {
        if !obs.gate_elo_delta.is_finite() {
            return Err(Error::NonFiniteGateValue {
                candidate_id: obs.candidate_id.clone(),
                field: "gate_elo_delta".to_string(),
            });
        }
        if !is_positive_finite(obs.gate_games_played) {
            return Err(Error::NonPositiveGateWeight {
                candidate_id: obs.candidate_id.clone(),
                value: obs.gate_games_played,
            });
        }
        let keys: Vec<String> = obs.features.keys().cloned().collect();
        if keys != feature_names {
            return Err(Error::InconsistentGateFeatures {
                candidate_id: obs.candidate_id.clone(),
            });
        }
        for (name, value) in &obs.features {
            if !value.is_finite() {
                return Err(Error::NonFiniteGateValue {
                    candidate_id: obs.candidate_id.clone(),
                    field: name.clone(),
                });
            }
        }
    }
    Ok(feature_names)
}

fn distinct_sorted<'a>(ids: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut ids: Vec<&str> = ids.collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Assigns each row to a cross-validation fold by its `group_id`, using
/// deterministic balanced GroupKFold. Takes a slice of references (not
/// owned `GateObservation`s) so the exact same function serves the full
/// dataset, an outer-CV training subset, and an inner-CV training subset
/// within that -- one fold-assignment implementation shared by every level
/// of [`GateModel::fit`]'s nested CV, so they can't silently disagree on how
/// a group maps to a fold.
///
/// Fold count is `min(cv_folds, num_groups)`. Groups are sorted by total
/// `gate_games_played` descending (ties broken by `fnv1a(group_id)`, then
/// `group_id` itself -- both deterministic and independent of input row
/// order), then placed one at a time onto whichever fold currently holds the
/// least total weight (ties broken by lowest fold index): a greedy
/// longest-processing-time-first bin-packing that keeps validation weight
/// close to balanced across folds. Unlike the previous `fnv1a(group_id) %
/// cv_folds` hash-mod scheme -- which could collide multiple groups into one
/// fold and leave another empty, with no guarantee `cv_folds_used` folds
/// were actually non-empty -- every fold here is guaranteed non-empty
/// whenever `num_groups >= cv_folds`: every fold starts at weight `0.0`,
/// every group's weight is `> 0.0` (`gate_games_played` is validated
/// positive), so the first `min(cv_folds, num_groups)` placements each land
/// on a still-empty fold before any fold receives a second group. Returns
/// the per-row fold id and the number of folds actually in play.
fn assign_folds(rows: &[&GateObservation], cv_folds: usize) -> (Vec<usize>, usize) {
    let mut group_weight: BTreeMap<&str, f64> = BTreeMap::new();
    for o in rows {
        *group_weight.entry(o.group_id.as_str()).or_insert(0.0) += o.gate_games_played;
    }
    let effective_folds = cv_folds.min(group_weight.len());

    let mut groups: Vec<(&str, f64)> = group_weight.into_iter().collect();
    groups.sort_by(|(a_id, a_weight), (b_id, b_weight)| {
        b_weight
            .total_cmp(a_weight)
            .then_with(|| fnv1a(a_id.as_bytes()).cmp(&fnv1a(b_id.as_bytes())))
            .then_with(|| a_id.cmp(b_id))
    });

    let mut fold_load = vec![0.0_f64; effective_folds];
    let mut group_fold: BTreeMap<&str, usize> = BTreeMap::new();
    for (group_id, weight) in groups {
        let fold = fold_load
            .iter()
            .enumerate()
            .min_by(|(ia, wa), (ib, wb)| wa.total_cmp(wb).then_with(|| ia.cmp(ib)))
            .map(|(idx, _)| idx)
            .expect("effective_folds > 0: every row contributes at least one distinct group");
        fold_load[fold] += weight;
        group_fold.insert(group_id, fold);
    }

    let fold_ids = rows
        .iter()
        .map(|o| {
            *group_fold
                .get(o.group_id.as_str())
                .expect("every row's group_id was inserted into group_fold above")
        })
        .collect();
    (fold_ids, effective_folds)
}

/// Per-feature weighted mean/std computed from a fixed set of rows (a
/// training fold, or the full dataset for the final refit) -- never from
/// rows being predicted, so standardization can't leak validation-set
/// information into the fit.
struct Standardizer {
    mean: Vec<f64>,
    std: Vec<f64>,
}

impl Standardizer {
    fn fit(rows: &[&GateObservation], feature_names: &[String], weights: &[f64]) -> Self {
        let sum_w: f64 = weights.iter().sum();
        let mean: Vec<f64> = feature_names
            .iter()
            .map(|name| {
                let s: f64 = rows
                    .iter()
                    .zip(weights)
                    .map(|(r, w)| w * r.features[name])
                    .sum();
                s / sum_w
            })
            .collect();
        let std: Vec<f64> = feature_names
            .iter()
            .enumerate()
            .map(|(j, name)| {
                let m = mean[j];
                let var: f64 = rows
                    .iter()
                    .zip(weights)
                    .map(|(r, w)| w * (r.features[name] - m).powi(2))
                    .sum::<f64>()
                    / sum_w;
                let sd = var.sqrt();
                // A constant feature has nothing to standardize by; every
                // row is already 0.0 after centering, so the divisor choice
                // is inert as long as it isn't 0.0/NaN.
                if sd > 1e-12 { sd } else { 1.0 }
            })
            .collect();
        Standardizer { mean, std }
    }

    fn transform(
        &self,
        features: &BTreeMap<String, f64>,
        feature_names: &[String],
    ) -> (Vec<f64>, Vec<String>, Vec<String>) {
        transform(features, feature_names, &self.mean, &self.std)
    }
}

/// Shared standardize-and-align logic for both a training row (where every
/// feature is always present, so `missing`/`unknown` come back empty) and a
/// [`GateQuery`] (where they might not be) -- one implementation so the two
/// call sites can't silently disagree on how alignment works.
fn transform(
    features: &BTreeMap<String, f64>,
    feature_names: &[String],
    mean: &[f64],
    std: &[f64],
) -> (Vec<f64>, Vec<String>, Vec<String>) {
    let unknown_features: Vec<String> = features
        .keys()
        .filter(|k| !feature_names.iter().any(|n| &n == k))
        .cloned()
        .collect();
    let mut missing_features = Vec::new();
    let x: Vec<f64> = feature_names
        .iter()
        .enumerate()
        .map(|(j, name)| match features.get(name) {
            Some(&v) => (v - mean[j]) / std[j],
            None => {
                missing_features.push(name.clone());
                0.0
            }
        })
        .collect();
    (x, missing_features, unknown_features)
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Result of fitting weighted ridge regression on one (fold or full-dataset)
/// training set, in already-standardized feature space.
struct RidgeFit {
    intercept: f64,
    coefficients: Vec<f64>,
    /// `(XᵀWX + lambda*I)⁻¹`, reused by [`predictive_variance`].
    m: Vec<Vec<f64>>,
    sigma2: f64,
    /// `1.0 / n_eff` -- the intercept's own posterior variance factor (the
    /// intercept is `y`'s weighted mean, a quantity fit from this same
    /// data, not a known constant), reused by [`predictive_variance`] so
    /// intercept uncertainty is never silently dropped from a prediction.
    intercept_variance_factor: f64,
}

/// Weighted ridge regression via the normal equations: minimizes
/// `sum_i w_i (y_i - x_i^T beta)^2 + lambda ||beta||^2`. `x_std` must already
/// be standardized (weighted mean 0 per column) so the intercept is simply
/// the weighted mean of `y` and `x_std` itself never needs centering.
///
/// `weights` is normalized here to mean `1.0` (sum `n`) before anything else
/// is computed from it: `A`'s diagonal is `~sum(weights)`, so without this,
/// `lambda`'s effective strength would depend on the *absolute* magnitude of
/// `gate_games_played` (tens vs. hundreds of games) rather than the relative
/// reliability it's meant to express -- with raw game counts, `A`'s diagonal
/// can dwarf every value in `GateModelConfig::lambda_grid`, silently
/// disabling regularization across most of the grid. Normalizing preserves
/// every pairwise weight *ratio* (so a 400-game row still counts 20x a
/// 20-game row) while pinning the grid's regularization strength to the
/// data's row count instead of its label-reliability units.
///
/// `sigma2` (residual variance) divides the weighted residual sum of squares
/// by `(n_eff - 1 - df)`: `n_eff` reuses [`effective_sample_size`] (Kish's
/// formula, already used elsewhere in this crate for weighted-data effective
/// sample size, and invariant to the weight-normalization above) rather than
/// a raw row count or weight sum; `df` is the ridge fit's effective degrees
/// of freedom for the standardized-feature coefficients,
/// `trace((XᵀWX + lambda*I)⁻¹ XᵀWX)`; and the extra `1` accounts for the
/// intercept (`y_mean`), which is also estimated from this same data rather
/// than known exactly. Clamped to a small positive floor so a degenerate
/// fold (`n_eff <= 1 + df`) can't produce a zero or negative variance.
fn fit_weighted_ridge(x_std: &[Vec<f64>], y: &[f64], weights: &[f64], lambda: f64) -> RidgeFit {
    let n = x_std.len();
    let p = x_std[0].len();
    let raw_sum_w: f64 = weights.iter().sum();
    let weights: Vec<f64> = weights.iter().map(|w| w * n as f64 / raw_sum_w).collect();
    let weights = &weights;
    let sum_w: f64 = weights.iter().sum(); // == n, kept symbolic for clarity below
    let y_mean = weights.iter().zip(y).map(|(w, yy)| w * yy).sum::<f64>() / sum_w;
    let y_centered: Vec<f64> = y.iter().map(|yy| yy - y_mean).collect();

    let mut a = vec![vec![0.0; p]; p];
    let mut b = vec![0.0; p];
    for i in 0..n {
        let w = weights[i];
        for j in 0..p {
            b[j] += w * x_std[i][j] * y_centered[i];
            for k in 0..p {
                a[j][k] += w * x_std[i][j] * x_std[i][k];
            }
        }
    }

    let m = invert_regularized(&a, lambda);
    let coefficients: Vec<f64> = (0..p).map(|j| dot(&m[j], &b)).collect();

    let mut rss = 0.0;
    for i in 0..n {
        let pred = y_mean + dot(&x_std[i], &coefficients);
        let resid = y[i] - pred;
        rss += weights[i] * resid * resid;
    }
    let sum_w_sq: f64 = weights.iter().map(|w| w * w).sum();
    let n_eff = effective_sample_size(sum_w, sum_w_sq);
    // trace(M*A): A and M are both symmetric, so this elementwise sum equals
    // the trace regardless of multiplication order.
    let df: f64 = m
        .iter()
        .zip(a.iter())
        .map(|(m_row, a_row)| dot(m_row, a_row))
        .sum();
    let sigma2 = rss / (n_eff - 1.0 - df).max(1e-6);
    let intercept_variance_factor = 1.0 / n_eff;

    RidgeFit {
        intercept: y_mean,
        coefficients,
        m,
        sigma2,
        intercept_variance_factor,
    }
}

/// Solves `(a + lambda*I)^{-1}` via Gauss-Jordan elimination with partial
/// pivoting on the augmented matrix `[a + lambda*I | I]`. `a` is p x p with p
/// bounded by the number of named gate features (small, single digits to
/// low tens) -- a hand-rolled solve here is simpler than a linear-algebra
/// dependency for one small inversion, matching this crate's existing
/// from-scratch-math convention (see `score.rs`). `lambda > 0.0` (enforced
/// by `validate_config`) makes `a + lambda*I` strictly positive definite, so
/// this never hits a zero pivot.
fn invert_regularized(a: &[Vec<f64>], lambda: f64) -> Vec<Vec<f64>> {
    let p = a.len();
    let mut aug = vec![vec![0.0; 2 * p]; p];
    for i in 0..p {
        for (j, &value) in a[i].iter().enumerate() {
            aug[i][j] = value + if i == j { lambda } else { 0.0 };
        }
        aug[i][p + i] = 1.0;
    }
    for col in 0..p {
        let pivot_row_index = (col..p)
            .max_by(|&r1, &r2| aug[r1][col].abs().total_cmp(&aug[r2][col].abs()))
            .expect("col..p is non-empty");
        aug.swap(col, pivot_row_index);
        let pivot = aug[col][col];
        for v in aug[col].iter_mut() {
            *v /= pivot;
        }
        // Cloned once per column so the elimination step below can read the
        // pivot row while mutating every other row -- `aug[col]` and
        // `aug[r]` alias the same `Vec<Vec<f64>>`, which the borrow checker
        // can't prove disjoint through indexing alone.
        let pivot_row_values = aug[col].clone();
        for (r, row) in aug.iter_mut().enumerate() {
            if r != col {
                let factor = row[col];
                if factor != 0.0 {
                    for (v, &pv) in row.iter_mut().zip(&pivot_row_values) {
                        *v -= factor * pv;
                    }
                }
            }
        }
    }
    (0..p).map(|i| aug[i][p..2 * p].to_vec()).collect()
}

/// Latent-mean predictive variance at `x_std`: `sigma2 * (intercept_variance_factor
/// plus x_std^T M x_std)`, the Bayesian-ridge posterior variance of
/// `intercept plus x*^T beta` (`M` = `(XᵀWX + lambda*I)⁻¹` doubles as the
/// Gaussian-prior posterior covariance of `beta` up to `sigma2`; the
/// intercept was fit separately as the weighted mean of `y`, with its own
/// posterior variance `sigma2 * intercept_variance_factor`, where
/// `intercept_variance_factor` is `1.0 / n_eff` -- see
/// [`fit_weighted_ridge`]'s doc comment). Standardized features are
/// weighted-centered, so `Cov(intercept, beta)` is `0.0` and the two terms
/// simply add, with no cross term. Without the `intercept_variance_factor`
/// term, a query at the training feature mean (`x_std == 0`, e.g. every
/// feature missing and imputed to its training mean) would report a
/// zero-width interval -- treating the intercept, a quantity estimated from
/// finite data, as if it were known exactly. Deliberately excludes the extra
/// unit variance a *prediction* interval for one new noisy gate run would
/// add -- this module reports uncertainty about the candidate's true
/// strength, not the sampling noise of a hypothetical future gate match (see
/// the module doc and `GatePrediction::interval_low`).
fn predictive_variance(
    m: &[Vec<f64>],
    x_std: &[f64],
    sigma2: f64,
    intercept_variance_factor: f64,
) -> f64 {
    let p = x_std.len();
    let mut q = 0.0;
    for j in 0..p {
        for k in 0..p {
            q += x_std[j] * m[j][k] * x_std[k];
        }
    }
    (sigma2 * (intercept_variance_factor + q)).max(0.0)
}

fn probability_positive(expected_elo: f64, variance: f64, sd: f64) -> f64 {
    if variance > 0.0 {
        standard_normal_cdf(expected_elo / sd)
    } else if expected_elo > 0.0 {
        1.0
    } else if expected_elo < 0.0 {
        0.0
    } else {
        0.5
    }
}

/// Standard normal CDF via Abramowitz & Stegun's rational erf approximation
/// (formula 7.1.26, max absolute error ~1.5e-7) -- avoids a new dependency
/// for one CDF evaluation, matching `score.rs`'s hand-rolled-math convention.
fn standard_normal_cdf(z: f64) -> f64 {
    0.5 * (1.0 + erf(z / std::f64::consts::SQRT_2))
}

fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let a1 = 0.254_829_592;
    let a2 = -0.284_496_736;
    let a3 = 1.421_413_741;
    let a4 = -1.453_152_027;
    let a5 = 1.061_405_429;
    let p = 0.327_591_1;
    let t = 1.0 / (1.0 + p * x);
    let poly = ((((a5 * t + a4) * t) + a3) * t + a2) * t + a1;
    let y = 1.0 - poly * t * (-x * x).exp();
    sign * y
}

/// One held-out prediction from [`run_folds`], kept for both
/// [`build_calibration`] and [`build_oof_table`] -- the same records feed
/// both, so the aggregate report and the per-candidate audit table can
/// never silently disagree about which predictions they're summarizing.
/// `outer_fold` is meaningless for [`select_lambda`]'s own (non-nested)
/// usage -- callers that only need the plain lambda selection discard the
/// predictions entirely rather than reading it there.
struct FoldPrediction {
    candidate_id: String,
    group_id: String,
    actual_elo: f64,
    predicted_elo: f64,
    stddev: f64,
    probability_positive: f64,
    actual_positive: bool,
    lambda: f64,
    gate_games_played: f64,
    outer_fold: usize,
}

/// Fits weighted ridge on `train_rows` at `lambda` (standardizing using
/// `train_rows`' own mean/std -- never `val_rows`', so standardization can't
/// leak validation-set information into the fit) and scores `val_rows`
/// against it. The common fit-then-score-held-out-rows step shared by
/// [`run_folds`]'s single level and by each outer fold of
/// [`nested_cross_validate`]'s inner level, so the two CVs can't silently
/// diverge in how a fold's model is fit or scored. Returns this fold's
/// weighted squared-error sum, weight sum, and every held-out prediction
/// (`outer_fold` left at `0`; [`run_folds`] stamps the real fold index).
fn fit_and_score_fold(
    train_rows: &[&GateObservation],
    val_rows: &[&GateObservation],
    feature_names: &[String],
    lambda: f64,
) -> (f64, f64, Vec<FoldPrediction>) {
    let train_weights: Vec<f64> = train_rows.iter().map(|o| o.gate_games_played).collect();
    let standardizer = Standardizer::fit(train_rows, feature_names, &train_weights);
    let x_train: Vec<Vec<f64>> = train_rows
        .iter()
        .map(|o| standardizer.transform(&o.features, feature_names).0)
        .collect();
    let y_train: Vec<f64> = train_rows.iter().map(|o| o.gate_elo_delta).collect();
    let fit = fit_weighted_ridge(&x_train, &y_train, &train_weights, lambda);

    let mut sq_err_sum = 0.0;
    let mut w_sum = 0.0;
    let mut predictions = Vec::new();
    for obs in val_rows {
        let (x, _missing, _unknown) = standardizer.transform(&obs.features, feature_names);
        let pred = fit.intercept + dot(&x, &fit.coefficients);
        let variance = predictive_variance(&fit.m, &x, fit.sigma2, fit.intercept_variance_factor);
        let sd = variance.sqrt();
        let w = obs.gate_games_played;
        sq_err_sum += w * (obs.gate_elo_delta - pred).powi(2);
        w_sum += w;
        predictions.push(FoldPrediction {
            candidate_id: obs.candidate_id.clone(),
            group_id: obs.group_id.clone(),
            actual_elo: obs.gate_elo_delta,
            predicted_elo: pred,
            stddev: sd,
            probability_positive: probability_positive(pred, variance, sd),
            actual_positive: obs.gate_elo_delta > 0.0,
            lambda,
            gate_games_played: w,
            outer_fold: 0,
        });
    }
    (sq_err_sum, w_sum, predictions)
}

/// Runs `effective_folds` folds of `rows`/`fold_ids`, calling
/// `lambda_for_fold` on each fold's own training rows to decide that fold's
/// lambda -- a constant closure (`|_| lambda`) gives the plain single-level
/// CV [`select_lambda`] uses; a closure that itself runs an inner CV gives
/// [`nested_cross_validate`]. One shared fold loop so the two can't
/// silently diverge in accumulation (weighted RMSE, predictions collected).
/// Stamps each returned [`FoldPrediction::outer_fold`] with this loop's own
/// fold index (meaningful only for [`nested_cross_validate`]'s outer level;
/// harmless and unread for [`select_lambda`]'s single-level usage, which
/// discards the predictions entirely). A fold with no training or no
/// validation rows contributes nothing (skipped, not an error) -- this can
/// happen at very small group counts and is an accepted engineering
/// approximation for this small-data regime rather than a case worth
/// hard-failing on. The returned fold count is a measurement, not the
/// `effective_folds` requested: it counts folds actually evaluated (skips
/// excluded), so callers never have to trust that `effective_folds` folds
/// were really all non-empty.
fn run_folds(
    rows: &[&GateObservation],
    fold_ids: &[usize],
    effective_folds: usize,
    feature_names: &[String],
    lambda_for_fold: impl Fn(&[&GateObservation]) -> f64,
) -> (usize, f64, Vec<FoldPrediction>) {
    let mut predictions = Vec::new();
    let mut sq_err_sum = 0.0;
    let mut w_sum = 0.0;
    let mut folds_used = 0;

    for fold in 0..effective_folds {
        let train_rows: Vec<&GateObservation> = (0..rows.len())
            .filter(|&i| fold_ids[i] != fold)
            .map(|i| rows[i])
            .collect();
        let val_rows: Vec<&GateObservation> = (0..rows.len())
            .filter(|&i| fold_ids[i] == fold)
            .map(|i| rows[i])
            .collect();
        if train_rows.is_empty() || val_rows.is_empty() {
            continue;
        }
        folds_used += 1;

        let lambda = lambda_for_fold(&train_rows);
        let (sq_err, w, fold_predictions) =
            fit_and_score_fold(&train_rows, &val_rows, feature_names, lambda);
        sq_err_sum += sq_err;
        w_sum += w;
        predictions.extend(fold_predictions.into_iter().map(|p| FoldPrediction {
            outer_fold: fold,
            ..p
        }));
    }

    let weighted_rmse = if w_sum > 0.0 {
        (sq_err_sum / w_sum).sqrt()
    } else {
        f64::INFINITY
    };
    (folds_used, weighted_rmse, predictions)
}

/// Plain, single-level group-aware lambda grid search: runs [`run_folds`]
/// once per `lambda_grid` candidate at a constant lambda, and keeps the one
/// with the lowest held-out weighted RMSE (ties broken by earliest lambda in
/// `lambda_grid`). Used both to pick the *deployed* model's lambda (over the
/// whole dataset) and, scoped to one outer fold's training rows, as
/// [`nested_cross_validate`]'s inner CV -- see [`GateModel::fit`]'s doc
/// comment for why those two uses must stay distinct.
fn select_lambda(
    rows: &[&GateObservation],
    feature_names: &[String],
    fold_ids: &[usize],
    effective_folds: usize,
    lambda_grid: &[f64],
) -> (f64, f64, Vec<FoldPrediction>) {
    let mut best: Option<(f64, f64, Vec<FoldPrediction>)> = None;
    for &lambda in lambda_grid {
        let (_folds_used, rmse, predictions) =
            run_folds(rows, fold_ids, effective_folds, feature_names, |_| lambda);
        let is_better = match &best {
            None => true,
            Some((_, best_rmse, _)) => rmse < *best_rmse,
        };
        if is_better {
            best = Some((lambda, rmse, predictions));
        }
    }
    best.expect("lambda_grid validated non-empty by validate_config")
}

/// Strongest (largest) lambda in the grid -- the fallback used when an outer
/// fold's own training rows don't have enough distinct groups to run an
/// honest inner CV of their own (see [`nested_cross_validate`]). Not an
/// arbitrary choice: with no data-driven way to pick among the grid for that
/// fold, the most conservative (most-regularized) option is the safer
/// default in this small-data-by-design module.
fn most_conservative_lambda(lambda_grid: &[f64]) -> f64 {
    lambda_grid
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max)
}

/// Nested group-aware cross-validation: for each outer fold (assigned over
/// the *entire* dataset), selects lambda using an inner CV scoped to only
/// that fold's own training rows (never its held-out rows), fits on the
/// full outer-training subset at that inner-selected lambda, and scores the
/// held-out outer-validation rows. Collects predictions across every outer
/// fold for an honest [`GateFitReport::weighted_rmse`]/`calibration` --
/// each held-out prediction is genuinely unseen by both the coefficients
/// *and* the lambda that produced them, unlike reusing [`select_lambda`]'s
/// own held-out predictions directly (see [`GateModel::fit`]'s doc comment
/// for why that would be optimistically biased).
///
/// When an outer fold's training rows have fewer than 2 distinct groups of
/// their own (possible at very small total group counts, e.g. the whole
/// dataset has only 2-3 groups), there is no honest way to run that fold's
/// own inner CV -- [`most_conservative_lambda`] is used instead of an
/// inner selection that couldn't hold anything out anyway.
///
/// Returns the number of outer folds actually evaluated (see
/// [`run_folds`]'s doc comment) alongside the weighted RMSE and predictions
/// -- [`GateModel::fit_with_validation`] reports this count directly as
/// `GateFitReport::cv_folds_used` rather than the outer fold assignment's
/// requested count, so that field is a measurement of what was actually
/// evaluated, not an assumption.
fn nested_cross_validate(
    all_rows: &[&GateObservation],
    feature_names: &[String],
    outer_fold_ids: &[usize],
    outer_effective_folds: usize,
    lambda_grid: &[f64],
    inner_cv_folds: usize,
) -> (usize, f64, Vec<FoldPrediction>) {
    run_folds(
        all_rows,
        outer_fold_ids,
        outer_effective_folds,
        feature_names,
        |outer_train_rows| {
            let (inner_fold_ids, inner_effective_folds) =
                assign_folds(outer_train_rows, inner_cv_folds);
            if inner_effective_folds >= 2 {
                select_lambda(
                    outer_train_rows,
                    feature_names,
                    &inner_fold_ids,
                    inner_effective_folds,
                    lambda_grid,
                )
                .0
            } else {
                most_conservative_lambda(lambda_grid)
            }
        },
    )
}

/// Bins `predictions` by `probability_positive` into `bins` equal-width
/// buckets over `[0, 1]` (same clamped-index convention as `eval.rs`'s
/// `CalibrationBin`), reporting each bin's empirical positive rate.
fn build_calibration(predictions: &[FoldPrediction], bins: usize) -> Vec<GateCalibrationBin> {
    let width = 1.0 / bins as f64;
    let mut counts = vec![0u64; bins];
    let mut positive_counts = vec![0u64; bins];
    for p in predictions {
        let idx = ((p.probability_positive / width) as usize).min(bins - 1);
        counts[idx] += 1;
        if p.actual_positive {
            positive_counts[idx] += 1;
        }
    }
    (0..bins)
        .map(|i| GateCalibrationBin {
            min_probability: i as f64 * width,
            max_probability: (i as f64 + 1.0) * width,
            num_observations: counts[i],
            empirical_positive_rate: if counts[i] > 0 {
                Some(positive_counts[i] as f64 / counts[i] as f64)
            } else {
                None
            },
        })
        .collect()
}

/// Maps every nested-CV out-of-fold [`FoldPrediction`] into a public
/// [`GateOofPrediction`] audit row (1:1, no deduplication -- a repeated
/// `candidate_id` in the input is not this crate's contract to enforce, so
/// it is preserved verbatim rather than collapsed through a map), then
/// sorts deterministically by `(outer_fold, group_id, candidate_id)` so the
/// same dataset always serializes identically regardless of input row order
/// (`fit_and_score_fold`'s internal `Vec` ordering follows fold iteration and
/// slice-filter order, neither of which is a stable contract on their own).
/// This determinism is over that sort key: two rows that are *fully*
/// identical on it (same fold, same group, same duplicate `candidate_id`)
/// keep whatever relative order `sort_by`'s stability gives them, which can
/// still trace back to input order -- harmless, since no field a caller
/// would sort or group by distinguishes them anyway.
fn build_oof_table(predictions: &[FoldPrediction], interval_z: f64) -> Vec<GateOofPrediction> {
    let mut rows: Vec<GateOofPrediction> = predictions
        .iter()
        .map(|p| GateOofPrediction {
            candidate_id: p.candidate_id.clone(),
            group_id: p.group_id.clone(),
            actual_elo: p.actual_elo,
            predicted_elo: p.predicted_elo,
            prediction_stddev: p.stddev,
            interval_low: p.predicted_elo - interval_z * p.stddev,
            interval_high: p.predicted_elo + interval_z * p.stddev,
            probability_positive: p.probability_positive,
            residual: p.actual_elo - p.predicted_elo,
            gate_games_played: p.gate_games_played,
            outer_fold: p.outer_fold,
            inner_selected_lambda: p.lambda,
        })
        .collect();
    rows.sort_by(|a, b| {
        a.outer_fold
            .cmp(&b.outer_fold)
            .then_with(|| a.group_id.cmp(&b.group_id))
            .then_with(|| a.candidate_id.cmp(&b.candidate_id))
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(candidate_id: &str, group_id: &str, x: f64, y: f64, games: f64) -> GateObservation {
        let mut features = BTreeMap::new();
        features.insert("x".to_string(), x);
        GateObservation {
            candidate_id: candidate_id.to_string(),
            group_id: group_id.to_string(),
            features,
            gate_elo_delta: y,
            gate_games_played: games,
        }
    }

    /// 12 rows, 12 distinct groups (>= cv_folds default of 5, so hash-mod
    /// fold assignment applies, not leave-one-group-out), a clean linear
    /// relationship `y = 10*x` plus a small deterministic zig-zag residual
    /// so ridge doesn't fit it perfectly -- enough rows for the default
    /// `required = max(6, 1 + 2) = 6` floor with headroom for CV folds to
    /// each get several rows.
    fn linear_dataset() -> Vec<GateObservation> {
        (0..12)
            .map(|i| {
                let x = i as f64;
                let noise = if i % 2 == 0 { 1.0 } else { -1.0 };
                obs(
                    &format!("c{i}"),
                    &format!("g{i}"),
                    x,
                    10.0 * x + noise,
                    100.0,
                )
            })
            .collect()
    }

    /// 4 distinct groups (< cv_folds default of 5, guaranteeing
    /// leave-one-group-out: every group becomes its own outer fold, so no
    /// outer fold's training or validation side can ever be empty), 3 rows
    /// each -- 12 rows total, satisfying `required = max(6, 1 + 2) = 6` with
    /// headroom. Same linear-plus-alternating-noise shape as
    /// `linear_dataset`, just grouped instead of one group per row.
    fn logo_dataset() -> Vec<GateObservation> {
        (0..12)
            .map(|i| {
                let x = i as f64;
                let noise = if i % 2 == 0 { 1.0 } else { -1.0 };
                obs(
                    &format!("c{i}"),
                    &format!("g{}", i / 3),
                    x,
                    10.0 * x + noise,
                    100.0,
                )
            })
            .collect()
    }

    #[test]
    fn standard_normal_cdf_matches_known_values() {
        assert!((standard_normal_cdf(0.0) - 0.5).abs() < 1e-6);
        assert!((standard_normal_cdf(1.96) - 0.975).abs() < 1e-3);
        assert!((standard_normal_cdf(-1.96) - 0.025).abs() < 1e-3);
    }

    #[test]
    fn invert_regularized_matches_hand_computed_2x2() {
        // a = [[2, 0], [0, 2]], lambda = 1 -> (a + I) = 3*I -> inverse = I/3.
        let a = vec![vec![2.0, 0.0], vec![0.0, 2.0]];
        let m = invert_regularized(&a, 1.0);
        assert!((m[0][0] - 1.0 / 3.0).abs() < 1e-9);
        assert!((m[1][1] - 1.0 / 3.0).abs() < 1e-9);
        assert!(m[0][1].abs() < 1e-9);
        assert!(m[1][0].abs() < 1e-9);
    }

    #[test]
    fn fit_weighted_ridge_matches_hand_derived_single_feature_case() {
        // Single centered feature x_std = [-1, 0, 1], y = [-2, 0, 2] (also
        // already centered), uniform weight 1.0. Normal equations:
        // A = x^T x = 2, b = x^T y = 4. beta = b / (A + lambda).
        let x = vec![vec![-1.0], vec![0.0], vec![1.0]];
        let y = vec![-2.0, 0.0, 2.0];
        let w = vec![1.0, 1.0, 1.0];
        let fit = fit_weighted_ridge(&x, &y, &w, 2.0);
        // beta = 4 / (2 + 2) = 1.0
        assert!((fit.coefficients[0] - 1.0).abs() < 1e-9);
        assert!((fit.intercept - 0.0).abs() < 1e-9);
        // sigma2 pinned to an exact hand-derived value (not just relative
        // invariance, which a scaled-but-still-wrong formula could also
        // satisfy): preds = x*beta = [-1, 0, 1], resid = y - preds =
        // [-1, 0, 1], rss = sum(resid^2) = 2. n_eff = sum_w^2/sum_w_sq =
        // 9/3 = 3. df = trace(M*A) = 0.25 * 2 = 0.5. The fitted intercept
        // (y_mean) also spends one degree of freedom of this same data, so
        // sigma2 = rss / (n_eff - 1 - df) = 2 / (3 - 1 - 0.5) = 2 / 1.5 =
        // 1.333333...
        assert!((fit.sigma2 - 2.0 / 1.5).abs() < 1e-9);
        // intercept_variance_factor = 1.0 / n_eff = 1.0 / 3.
        assert!((fit.intercept_variance_factor - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn fit_weighted_ridge_is_invariant_to_uniform_weight_magnitude_at_a_fixed_lambda() {
        // Weights are normalized to mean 1.0 before entering the normal
        // equations (see fit_weighted_ridge's doc comment), specifically so
        // that gate_games_played=[100,100,100] and gate_games_played=[1,1,1]
        // -- equally reliable rows, just different absolute game counts --
        // produce the identical fit at the *same* lambda. Without that
        // normalization this would require scaling lambda by the same
        // factor as the weights, making the lambda_grid's meaning depend on
        // whatever units the caller's gate_games_played happens to use.
        let x = vec![vec![-1.0], vec![0.0], vec![1.0]];
        let y = vec![-2.0, 0.5, 2.0]; // slightly noisy, not perfectly linear
        let uniform_1 = fit_weighted_ridge(&x, &y, &[1.0, 1.0, 1.0], 2.0);
        let uniform_100 = fit_weighted_ridge(&x, &y, &[100.0, 100.0, 100.0], 2.0);
        assert!((uniform_1.coefficients[0] - uniform_100.coefficients[0]).abs() < 1e-9);
        assert!((uniform_1.intercept - uniform_100.intercept).abs() < 1e-9);
        assert!((uniform_1.sigma2 - uniform_100.sigma2).abs() < 1e-9);
        // n_eff (and so intercept_variance_factor = 1.0/n_eff) is also
        // homogeneous-degree-0 in the weights -- must survive the same
        // magnitude scaling as everything else this test pins.
        assert!(
            (uniform_1.intercept_variance_factor - uniform_100.intercept_variance_factor).abs()
                < 1e-9
        );
    }

    #[test]
    fn fit_weighted_ridge_preserves_relative_weight_ratios() {
        // Doubling every weight together with an unequal ratio (2:1, not
        // uniform) must give the same fit as the un-doubled ratio -- only
        // the *relative* reliability between rows should matter, not either
        // row's absolute gate_games_played value.
        let x = vec![vec![-1.0], vec![0.0], vec![1.0]];
        let y = vec![-2.0, 0.5, 2.0];
        let ratio_2_1 = fit_weighted_ridge(&x, &y, &[2.0, 2.0, 1.0], 2.0);
        let same_ratio_scaled = fit_weighted_ridge(&x, &y, &[200.0, 200.0, 100.0], 2.0);
        assert!((ratio_2_1.coefficients[0] - same_ratio_scaled.coefficients[0]).abs() < 1e-9);
        assert!((ratio_2_1.intercept - same_ratio_scaled.intercept).abs() < 1e-9);
    }

    #[test]
    fn predictive_variance_grows_further_from_training_data() {
        let m = invert_regularized(&[vec![4.0]], 1.0); // A=4, lambda=1 -> M = 1/5
        let near = predictive_variance(&m, &[0.5], 1.0, 0.0);
        let far = predictive_variance(&m, &[5.0], 1.0, 0.0);
        assert!(far > near);
    }

    #[test]
    fn predictive_variance_includes_intercept_uncertainty_even_at_x_zero() {
        // At the training feature mean (x_std == 0) the x^T M x term
        // vanishes entirely -- P0-1's fix: variance must not collapse to
        // 0.0 there, since the intercept itself is fit from finite data.
        let m = invert_regularized(&[vec![4.0]], 1.0);
        let at_mean = predictive_variance(&m, &[0.0], 1.0, 0.2);
        assert!(at_mean.is_finite());
        assert!(at_mean > 0.0);
        assert!((at_mean - 0.2).abs() < 1e-12); // sigma2 * (0.2 + 0) == 0.2
    }

    #[test]
    fn fit_selects_a_lambda_from_the_grid_and_reports_positive_rmse() {
        let output = GateModel::fit(&linear_dataset(), &GateModelConfig::default()).unwrap();
        assert!(
            GateModelConfig::default()
                .lambda_grid
                .contains(&output.report.selected_lambda)
        );
        assert!(output.report.weighted_rmse.is_finite());
        assert!(output.report.weighted_rmse >= 0.0);
        assert_eq!(output.report.num_observations, 12);
        assert_eq!(output.report.num_groups, 12);
        assert_eq!(output.report.calibration.len(), 5);
    }

    #[test]
    fn fit_is_deterministic() {
        let data = linear_dataset();
        let a = GateModel::fit(&data, &GateModelConfig::default()).unwrap();
        let b = GateModel::fit(&data, &GateModelConfig::default()).unwrap();
        assert_eq!(a.report.selected_lambda, b.report.selected_lambda);
        assert!((a.report.weighted_rmse - b.report.weighted_rmse).abs() < 1e-12);
    }

    #[test]
    fn fit_selected_lambda_is_invariant_to_the_uniform_magnitude_of_gate_games_played() {
        // Same relationship, same relative reliability (uniform across
        // rows) -- only the absolute gate_games_played units differ (1 vs.
        // 100). The lambda_grid's regularization strength must track row
        // count, not whatever units gate_games_played happens to use, or
        // the grid search silently regularizes less as recorded game counts
        // grow (see fit_weighted_ridge's weight-normalization doc comment).
        let mut few_games = linear_dataset();
        for o in &mut few_games {
            o.gate_games_played = 1.0;
        }
        let mut many_games = linear_dataset();
        for o in &mut many_games {
            o.gate_games_played = 100.0;
        }
        let a = GateModel::fit(&few_games, &GateModelConfig::default()).unwrap();
        let b = GateModel::fit(&many_games, &GateModelConfig::default()).unwrap();
        assert_eq!(a.report.selected_lambda, b.report.selected_lambda);
        assert!((a.report.weighted_rmse - b.report.weighted_rmse).abs() < 1e-9);
    }

    #[test]
    fn predict_on_a_strongly_positive_candidate_favors_positive_probability() {
        let output = GateModel::fit(&linear_dataset(), &GateModelConfig::default()).unwrap();
        let mut features = BTreeMap::new();
        features.insert("x".to_string(), 20.0); // well beyond the training range, strongly positive trend
        let prediction = output.model.predict(&GateQuery { features });
        assert!(prediction.expected_elo > 0.0);
        assert!(prediction.probability_positive > 0.5);
        assert!(prediction.interval_low < prediction.expected_elo);
        assert!(prediction.interval_high > prediction.expected_elo);
        assert!(prediction.missing_features.is_empty());
        assert!(prediction.unknown_features.is_empty());
    }

    #[test]
    fn predict_reports_missing_and_unknown_features_without_erroring() {
        let output = GateModel::fit(&linear_dataset(), &GateModelConfig::default()).unwrap();
        let mut features = BTreeMap::new();
        features.insert("y_never_trained_on".to_string(), 1.0); // unknown; "x" is missing
        let prediction = output.model.predict(&GateQuery { features });
        assert_eq!(prediction.missing_features, vec!["x".to_string()]);
        assert_eq!(
            prediction.unknown_features,
            vec!["y_never_trained_on".to_string()]
        );
        assert!(prediction.expected_elo.is_finite()); // still produces a (documented, degraded) answer
    }

    #[test]
    fn missing_feature_prediction_equals_explicit_prediction_at_the_training_mean() {
        // Pins the imputation claim exactly, not just "doesn't error": a
        // missing feature must produce a numerically identical prediction to
        // explicitly supplying that feature's own training-set mean (the
        // documented behavior of GatePrediction::missing_features).
        let output = GateModel::fit(&linear_dataset(), &GateModelConfig::default()).unwrap();
        let missing = output.model.predict(&GateQuery {
            features: BTreeMap::new(),
        });
        let mut at_mean = BTreeMap::new();
        at_mean.insert("x".to_string(), 5.5); // linear_dataset()'s "x" is 0..=11, mean 5.5
        let explicit = output.model.predict(&GateQuery { features: at_mean });

        assert_eq!(missing.missing_features, vec!["x".to_string()]);
        assert!((missing.expected_elo - explicit.expected_elo).abs() < 1e-9);
        assert!((missing.interval_low - explicit.interval_low).abs() < 1e-9);
        assert!((missing.interval_high - explicit.interval_high).abs() < 1e-9);
        assert!((missing.probability_positive - explicit.probability_positive).abs() < 1e-9);
    }

    #[test]
    fn predictive_variance_at_the_training_feature_mean_is_finite_and_positive() {
        // P0-1 regression guard at the GateModel::predict level: a query at
        // exactly the training feature mean standardizes to x_std == 0, so
        // the old `sigma2 * x^T M x` formula collapsed to 0.0 there --
        // treating the fitted intercept as known exactly rather than as an
        // estimate with its own uncertainty.
        let output = GateModel::fit(&linear_dataset(), &GateModelConfig::default()).unwrap();
        let mut features = BTreeMap::new();
        features.insert("x".to_string(), 5.5); // linear_dataset()'s "x" mean
        let prediction = output.model.predict(&GateQuery { features });
        let half_width = prediction.interval_high - prediction.expected_elo;
        assert!(half_width.is_finite());
        assert!(half_width > 0.0);
    }

    #[test]
    fn missing_all_features_query_does_not_get_a_zero_width_interval() {
        // Same regression as above, reached through the documented
        // missing-feature imputation path instead of an explicit mean value.
        let output = GateModel::fit(&linear_dataset(), &GateModelConfig::default()).unwrap();
        let prediction = output.model.predict(&GateQuery {
            features: BTreeMap::new(),
        });
        assert_eq!(prediction.missing_features, vec!["x".to_string()]);
        assert!(prediction.interval_high > prediction.interval_low);
        assert!((prediction.interval_high - prediction.interval_low).is_finite());
    }

    #[test]
    fn fit_falls_back_to_conservative_lambda_when_an_outer_folds_training_rows_have_too_few_groups_for_inner_cv()
     {
        // Only 2 total groups -- the outer CV falls back to
        // leave-one-group-out (2 outer folds), so each outer fold's own
        // training rows come from just the *other* single group: too few
        // distinct groups (1) to run an honest inner CV of its own. This
        // exercises nested_cross_validate's most_conservative_lambda
        // fallback rather than panicking or acting on an inner grid search
        // that couldn't hold anything out anyway.
        let mut data = Vec::new();
        for i in 0..6 {
            data.push(obs(
                &format!("a{i}"),
                "gA",
                i as f64,
                10.0 * i as f64,
                100.0,
            ));
        }
        for i in 0..6 {
            data.push(obs(
                &format!("b{i}"),
                "gB",
                i as f64 + 6.0,
                10.0 * (i as f64 + 6.0),
                100.0,
            ));
        }
        let output = GateModel::fit(&data, &GateModelConfig::default()).unwrap();
        assert_eq!(output.report.num_groups, 2);
        assert_eq!(output.report.cv_folds_used, 2);
        assert!(output.report.weighted_rmse.is_finite());
        assert_eq!(output.report.calibration.len(), 5);
    }

    #[test]
    fn assign_folds_never_splits_a_group_across_folds() {
        // 3 groups, several rows each, cv_folds requested at 5 -- falls back
        // to leave-one-group-out (3 folds), one per group.
        let data = [
            obs("c0", "gA", 0.0, 1.0, 10.0),
            obs("c1", "gA", 1.0, 2.0, 10.0),
            obs("c2", "gB", 2.0, 3.0, 10.0),
            obs("c3", "gB", 3.0, 4.0, 10.0),
            obs("c4", "gC", 4.0, 5.0, 10.0),
            obs("c5", "gC", 5.0, 6.0, 10.0),
        ];
        let rows: Vec<&GateObservation> = data.iter().collect();
        let (fold_ids, effective_folds) = assign_folds(&rows, 5);
        assert_eq!(effective_folds, 3);
        // Every row sharing a group_id must land in the same fold.
        assert_eq!(fold_ids[0], fold_ids[1]); // both gA
        assert_eq!(fold_ids[2], fold_ids[3]); // both gB
        assert_eq!(fold_ids[4], fold_ids[5]); // both gC
        // And distinct groups must land in distinct folds under LOGO.
        assert_ne!(fold_ids[0], fold_ids[2]);
        assert_ne!(fold_ids[2], fold_ids[4]);
    }

    #[test]
    fn assign_folds_balanced_branch_also_keeps_a_group_together() {
        // 5 distinct groups (== cv_folds), 4 rows each -- enough distinct
        // groups that fold count is cv_folds itself
        // (min(cv_folds, num_groups) == cv_folds), not the
        // fewer-groups-than-folds case where every group trivially gets its
        // own fold.
        let data: Vec<GateObservation> = (0..20)
            .map(|i| {
                obs(
                    &format!("c{i}"),
                    &format!("g{}", i % 5),
                    i as f64,
                    i as f64,
                    10.0,
                )
            })
            .collect();
        let rows: Vec<&GateObservation> = data.iter().collect();
        let (fold_ids, effective_folds) = assign_folds(&rows, 5);
        assert_eq!(effective_folds, 5);
        for group in 0..5 {
            let rows_in_group: Vec<usize> = (0..20).filter(|&i| i % 5 == group).collect();
            let folds: Vec<usize> = rows_in_group.iter().map(|&i| fold_ids[i]).collect();
            assert!(
                folds.iter().all(|&f| f == folds[0]),
                "group g{group}'s rows landed in different folds: {folds:?}"
            );
        }
    }

    #[test]
    fn every_requested_fold_is_non_empty_when_enough_groups_exist() {
        // 12 distinct groups, default cv_folds == 5: fold count is
        // min(cv_folds, num_groups) == 5, and balanced GroupKFold guarantees
        // every one of those 5 folds gets at least one group. Regression
        // guard for the old `fnv1a(group_id) % cv_folds` scheme, where a
        // hash collision could leave a fold empty and `run_folds` would
        // silently skip it -- so a "5-fold" report could actually reflect
        // only 3-4 evaluated folds.
        let data = linear_dataset();
        let output = GateModel::fit_with_validation(&data, &GateModelConfig::default()).unwrap();
        assert_eq!(output.report.cv_folds_used, 5);

        let observed_folds: std::collections::BTreeSet<usize> = output
            .oof_predictions
            .iter()
            .map(|p| p.outer_fold)
            .collect();
        let expected_folds: std::collections::BTreeSet<usize> = (0..5).collect();
        assert_eq!(observed_folds, expected_folds);

        // Every OOF observation still appears exactly once in this branch
        // too (not just under leave-one-group-out, covered elsewhere).
        assert_eq!(output.oof_predictions.len(), data.len());
        let mut got: Vec<&str> = output
            .oof_predictions
            .iter()
            .map(|p| p.candidate_id.as_str())
            .collect();
        got.sort_unstable();
        let mut expected: Vec<&str> = data.iter().map(|o| o.candidate_id.as_str()).collect();
        expected.sort_unstable();
        assert_eq!(got, expected);
    }

    #[test]
    fn assign_folds_balances_fold_weight_within_the_largest_group_weight() {
        // 5 "whale" groups worth 1000 games each plus 8 "small" groups
        // worth 10 games each, cv_folds requested at 4: more whales than
        // folds forces at least one fold to double up (a naive hash-mod
        // split could instead dump several whales into the same fold and
        // leave another empty). The greedy min-load GroupKFold has a
        // provable bound -- max_fold_weight <= total_weight/folds +
        // max_group_weight -- checked here directly against a deliberately
        // skewed fixture, not by re-implementing the algorithm and checking
        // it agrees with itself. With 5 whales over 4 folds the bound is
        // actually stressed (max fold ends up double a single-whale fold's
        // weight, not equal to every other fold's).
        let mut data = Vec::new();
        for i in 0..5 {
            data.push(obs(
                &format!("whale{i}"),
                &format!("gWhale{i}"),
                i as f64,
                i as f64,
                1000.0,
            ));
        }
        for i in 0..8 {
            data.push(obs(
                &format!("small{i}"),
                &format!("gSmall{i}"),
                i as f64,
                i as f64,
                10.0,
            ));
        }
        let rows: Vec<&GateObservation> = data.iter().collect();
        let (fold_ids, effective_folds) = assign_folds(&rows, 4);
        assert_eq!(effective_folds, 4);

        let mut fold_weight = vec![0.0; effective_folds];
        for (i, o) in data.iter().enumerate() {
            fold_weight[fold_ids[i]] += o.gate_games_played;
        }
        let total_weight: f64 = data.iter().map(|o| o.gate_games_played).sum();
        let max_group_weight = 1000.0;
        let bound = total_weight / effective_folds as f64 + max_group_weight;
        let max_fold_weight = fold_weight.iter().copied().fold(f64::MIN, f64::max);
        let min_fold_weight = fold_weight.iter().copied().fold(f64::MAX, f64::min);
        assert!(
            max_fold_weight > min_fold_weight + 1e-9,
            "fixture didn't stress the bound: every fold landed at the same weight {fold_weight:?}"
        );
        for (fold, &w) in fold_weight.iter().enumerate() {
            assert!(
                w <= bound + 1e-9,
                "fold {fold} weight {w} exceeds the LPT bound {bound}"
            );
            assert!(w > 0.0, "fold {fold} is empty");
        }
    }

    #[test]
    fn assign_folds_is_deterministic_and_input_order_invariant() {
        // Multi-row groups with clearly separated (non-round) total
        // weights, not one row per group: `assign_folds` accumulates each
        // group's weight via `+=` in row-iteration order, and float
        // addition isn't associative, so a single-row-per-group fixture
        // would never exercise that accumulation at all. Group totals are
        // spaced far enough apart (multiples of ~30) that summation order
        // can't perturb which group has the larger total by anywhere near
        // enough to flip the sort -- this proves order-invariance on the
        // path that could actually break it, without becoming a flaky
        // near-tie test.
        let mut data = Vec::new();
        let per_group_row_weights = [
            [3.1, 3.0, 2.9],    // gA total ~9.0
            [13.3, 13.0, 12.7], // gB total ~39.0
            [23.4, 23.0, 22.6], // gC total ~69.0
            [33.5, 33.0, 32.5], // gD total ~99.0
        ];
        for (g, weights) in per_group_row_weights.iter().enumerate() {
            for (r, &w) in weights.iter().enumerate() {
                data.push(obs(
                    &format!("c{g}_{r}"),
                    &format!("g{g}"),
                    g as f64,
                    g as f64,
                    w,
                ));
            }
        }
        let rows: Vec<&GateObservation> = data.iter().collect();
        let (fold_ids_a, effective_a) = assign_folds(&rows, 3);

        // Scramble both cross-group order and each group's own row order.
        let mut shuffled = data.clone();
        shuffled.reverse();
        shuffled.swap(0, 6);
        shuffled.swap(2, 9);
        shuffled.swap(4, 11);
        let shuffled_rows: Vec<&GateObservation> = shuffled.iter().collect();
        let (fold_ids_b, effective_b) = assign_folds(&shuffled_rows, 3);

        assert_eq!(effective_a, effective_b);

        let group_fold_a: BTreeMap<&str, usize> = data
            .iter()
            .zip(&fold_ids_a)
            .map(|(o, &f)| (o.group_id.as_str(), f))
            .collect();
        let group_fold_b: BTreeMap<&str, usize> = shuffled
            .iter()
            .zip(&fold_ids_b)
            .map(|(o, &f)| (o.group_id.as_str(), f))
            .collect();
        assert_eq!(group_fold_a, group_fold_b);
    }

    #[test]
    fn fit_rejects_a_single_group() {
        let data: Vec<GateObservation> = (0..10)
            .map(|i| obs(&format!("c{i}"), "only_group", i as f64, i as f64, 10.0))
            .collect();
        let err = GateModel::fit(&data, &GateModelConfig::default()).unwrap_err();
        assert!(matches!(
            err,
            Error::InsufficientGateGroups { num_groups: 1 }
        ));
    }

    #[test]
    fn fit_rejects_too_few_observations_for_the_feature_count() {
        let data = vec![
            obs("c0", "g0", 0.0, 0.0, 10.0),
            obs("c1", "g1", 1.0, 1.0, 10.0),
        ];
        let err = GateModel::fit(&data, &GateModelConfig::default()).unwrap_err();
        assert!(matches!(
            err,
            Error::InsufficientGateObservations {
                num_observations: 2,
                ..
            }
        ));
    }

    #[test]
    fn fit_rejects_inconsistent_feature_sets() {
        let mut data = linear_dataset();
        data[3].features.insert("extra".to_string(), 1.0);
        let err = GateModel::fit(&data, &GateModelConfig::default()).unwrap_err();
        assert!(matches!(err, Error::InconsistentGateFeatures { .. }));
    }

    #[test]
    fn fit_rejects_non_finite_label() {
        let mut data = linear_dataset();
        data[0].gate_elo_delta = f64::NAN;
        let err = GateModel::fit(&data, &GateModelConfig::default()).unwrap_err();
        assert!(matches!(err, Error::NonFiniteGateValue { .. }));
    }

    #[test]
    fn fit_rejects_non_positive_weight() {
        let mut data = linear_dataset();
        data[0].gate_games_played = 0.0;
        let err = GateModel::fit(&data, &GateModelConfig::default()).unwrap_err();
        assert!(matches!(err, Error::NonPositiveGateWeight { .. }));
    }

    #[test]
    fn fit_rejects_invalid_config() {
        let data = linear_dataset();
        let err = GateModel::fit(
            &data,
            &GateModelConfig {
                lambda_grid: vec![],
                ..GateModelConfig::default()
            },
        )
        .unwrap_err();
        assert!(matches!(err, Error::InvalidConfig { .. }));

        let err = GateModel::fit(
            &data,
            &GateModelConfig {
                lambda_grid: vec![0.0],
                ..GateModelConfig::default()
            },
        )
        .unwrap_err();
        assert!(matches!(err, Error::InvalidConfig { .. }));
    }

    #[test]
    fn calibration_bins_are_always_the_configured_length() {
        let output = GateModel::fit(
            &linear_dataset(),
            &GateModelConfig {
                calibration_bins: 4,
                ..GateModelConfig::default()
            },
        )
        .unwrap();
        assert_eq!(output.report.calibration.len(), 4);
    }

    #[test]
    fn oof_predictions_cover_every_observation_exactly_once_under_leave_one_group_out() {
        let data = logo_dataset();
        let output = GateModel::fit_with_validation(&data, &GateModelConfig::default()).unwrap();
        assert_eq!(output.oof_predictions.len(), data.len());

        let mut got: Vec<&str> = output
            .oof_predictions
            .iter()
            .map(|p| p.candidate_id.as_str())
            .collect();
        got.sort_unstable();
        let mut expected: Vec<&str> = data.iter().map(|o| o.candidate_id.as_str()).collect();
        expected.sort_unstable();
        assert_eq!(got, expected);
    }

    #[test]
    fn oof_predictions_never_train_on_their_own_group_under_leave_one_group_out() {
        // Under LOGO every outer fold's held-out rows come from exactly one
        // group; proving each fold's rows share a single group_id directly
        // proves that group's own rows never appeared in that fold's
        // training set (fit_and_score_fold's train_rows is precisely "every
        // row not in this fold").
        let data = logo_dataset();
        let output = GateModel::fit_with_validation(&data, &GateModelConfig::default()).unwrap();
        let mut by_fold: BTreeMap<usize, Vec<&str>> = BTreeMap::new();
        for p in &output.oof_predictions {
            by_fold.entry(p.outer_fold).or_default().push(&p.group_id);
        }
        for (fold, groups) in &by_fold {
            assert!(
                groups.iter().all(|g| *g == groups[0]),
                "fold {fold} mixes groups: {groups:?}"
            );
        }
    }

    #[test]
    fn inner_selected_lambda_is_uniform_within_an_outer_fold() {
        let data = logo_dataset();
        let output = GateModel::fit_with_validation(&data, &GateModelConfig::default()).unwrap();
        let mut by_fold: BTreeMap<usize, Vec<f64>> = BTreeMap::new();
        for p in &output.oof_predictions {
            by_fold
                .entry(p.outer_fold)
                .or_default()
                .push(p.inner_selected_lambda);
        }
        for (fold, lambdas) in &by_fold {
            assert!(
                lambdas.iter().all(|&l| (l - lambdas[0]).abs() < 1e-12),
                "fold {fold} used more than one lambda: {lambdas:?}"
            );
        }
    }

    #[test]
    fn weighted_rmse_is_exactly_reproducible_from_oof_predictions() {
        let output =
            GateModel::fit_with_validation(&linear_dataset(), &GateModelConfig::default()).unwrap();
        let sq_err_sum: f64 = output
            .oof_predictions
            .iter()
            .map(|p| p.gate_games_played * p.residual * p.residual)
            .sum();
        let w_sum: f64 = output
            .oof_predictions
            .iter()
            .map(|p| p.gate_games_played)
            .sum();
        let recomputed = (sq_err_sum / w_sum).sqrt();
        assert!((recomputed - output.report.weighted_rmse).abs() < 1e-9);
    }

    #[test]
    fn calibration_bins_are_exactly_reproducible_from_oof_predictions() {
        let output =
            GateModel::fit_with_validation(&linear_dataset(), &GateModelConfig::default()).unwrap();
        let bins = output.report.calibration.len();
        let width = 1.0 / bins as f64;
        let mut counts = vec![0u64; bins];
        let mut positive_counts = vec![0u64; bins];
        for p in &output.oof_predictions {
            let idx = ((p.probability_positive / width) as usize).min(bins - 1);
            counts[idx] += 1;
            if p.actual_elo > 0.0 {
                positive_counts[idx] += 1;
            }
        }
        for (i, bin) in output.report.calibration.iter().enumerate() {
            assert_eq!(bin.num_observations, counts[i]);
            let expected_rate = if counts[i] > 0 {
                Some(positive_counts[i] as f64 / counts[i] as f64)
            } else {
                None
            };
            assert_eq!(bin.empirical_positive_rate, expected_rate);
        }
    }

    #[test]
    fn interval_width_matches_the_configured_interval_level() {
        let config = GateModelConfig::default();
        let output = GateModel::fit_with_validation(&linear_dataset(), &config).unwrap();

        let expected_level = 2.0 * standard_normal_cdf(config.interval_z) - 1.0;
        assert!((output.interval_level - expected_level).abs() < 1e-9);

        for p in &output.oof_predictions {
            assert!(p.interval_low <= p.predicted_elo);
            assert!(p.predicted_elo <= p.interval_high);
            if p.prediction_stddev > 0.0 {
                let implied_z = (p.interval_high - p.interval_low) / (2.0 * p.prediction_stddev);
                assert!((implied_z - config.interval_z).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn oof_predictions_are_ordered_deterministically_regardless_of_input_order() {
        let data = linear_dataset();
        let mut reversed = data.clone();
        reversed.reverse();

        let a = GateModel::fit_with_validation(&data, &GateModelConfig::default()).unwrap();
        let b = GateModel::fit_with_validation(&reversed, &GateModelConfig::default()).unwrap();

        // Sort-key sequence must match regardless of input row order --
        // comparing full rows with `==` would also compare floating-point
        // predictions, which summation-order (a real effect of the
        // differently-ordered fold slices) could nudge by a few ULPs even
        // though the ordering itself is exactly reproducible.
        let keys = |predictions: &[GateOofPrediction]| -> Vec<(usize, String, String)> {
            predictions
                .iter()
                .map(|p| (p.outer_fold, p.group_id.clone(), p.candidate_id.clone()))
                .collect()
        };
        assert_eq!(keys(&a.oof_predictions), keys(&b.oof_predictions));

        assert_eq!(a.oof_predictions.len(), b.oof_predictions.len());
        for (pa, pb) in a.oof_predictions.iter().zip(&b.oof_predictions) {
            assert_eq!(pa.candidate_id, pb.candidate_id);
            assert!((pa.predicted_elo - pb.predicted_elo).abs() < 1e-9);
        }
    }

    #[test]
    fn duplicate_candidate_ids_are_preserved_not_collapsed() {
        // Two rows from different groups deliberately share a candidate_id
        // -- lineprior doesn't treat candidate_id as unique, so both rows
        // must survive in oof_predictions rather than being deduplicated
        // through a map.
        let mut data = logo_dataset();
        data[0].candidate_id = "dup".to_string(); // group g0
        data[3].candidate_id = "dup".to_string(); // group g1
        let output = GateModel::fit_with_validation(&data, &GateModelConfig::default()).unwrap();
        assert_eq!(output.oof_predictions.len(), data.len());
        let dup_count = output
            .oof_predictions
            .iter()
            .filter(|p| p.candidate_id == "dup")
            .count();
        assert_eq!(dup_count, 2);
    }
}
