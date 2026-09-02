use crate::{Observation, Outcome, PriorAction, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for extracting contiguous action windows from histories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacroActionConfig {
    pub min_length: usize,
    pub max_length: usize,
    pub min_count: u64,
}

impl Default for MacroActionConfig {
    fn default() -> Self {
        Self {
            min_length: 2,
            max_length: 4,
            min_count: 2,
        }
    }
}

/// A reusable contiguous action sequence starting at a state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MacroAction {
    pub state: String,
    pub actions: Vec<String>,
    pub count: u64,
    pub success_rate: Option<f64>,
    pub prior: f64,
}

/// Extracts macro-actions from ordered histories. This is deliberately eager:
/// callers must provide a bounded slice because sequence windows require
/// retaining each sequence, unlike the streaming single-step builder.
pub fn build_macro_actions(
    observations: &[Observation],
    config: MacroActionConfig,
) -> Result<Vec<MacroAction>> {
    if observations.is_empty() {
        return Err(crate::Error::NoObservations);
    }
    if config.min_length == 0 || config.max_length < config.min_length || config.min_count == 0 {
        return Err(crate::Error::InvalidConfig {
            message: "macro action lengths/count are invalid".into(),
        });
    }
    let mut rows = observations.to_vec();
    rows.sort_by(|a, b| a.sequence_id.cmp(&b.sequence_id).then(a.step.cmp(&b.step)));
    let mut stats: HashMap<(String, Vec<String>), (u64, f64, f64)> = HashMap::new();
    let mut start = 0;
    while start < rows.len() {
        let end = rows[start..]
            .iter()
            .position(|r| r.sequence_id != rows[start].sequence_id)
            .map_or(rows.len(), |n| start + n);
        for i in start..end {
            for len in config.min_length..=config.max_length {
                if i + len > end {
                    break;
                }
                let actions = rows[i..i + len]
                    .iter()
                    .map(|r| r.action.clone())
                    .collect::<Vec<_>>();
                let successes = rows[i..i + len]
                    .iter()
                    .map(|r| match r.outcome {
                        Outcome::Success => 1.0,
                        Outcome::Draw => 0.5,
                        _ => 0.0,
                    })
                    .sum::<f64>();
                let trials = rows[i..i + len]
                    .iter()
                    .filter(|r| !matches!(r.outcome, Outcome::Unknown))
                    .count() as f64;
                let key = (rows[i].state.clone(), actions);
                let entry = stats.entry(key).or_default();
                entry.0 += 1;
                entry.1 += successes;
                entry.2 += trials;
            }
        }
        start = end;
    }
    let mut kept: Vec<(String, Vec<String>, u64, Option<f64>)> = stats
        .into_iter()
        .filter(|(_, (count, _, _))| *count >= config.min_count)
        .map(|((state, actions), (count, successes, trials))| {
            (
                state,
                actions,
                count,
                (trials > 0.0).then_some(successes / trials),
            )
        })
        .collect();
    kept.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let total = kept.iter().map(|x| x.2).sum::<u64>() as f64;
    Ok(kept
        .into_iter()
        .map(|(state, actions, count, success_rate)| MacroAction {
            state,
            actions,
            count,
            success_rate,
            prior: count as f64 / total,
        })
        .collect())
}

/// Converts macro-action suggestions to the same candidate shape used by a
/// normal prior query. The joined action is explicit and never invented.
pub fn macro_action_candidates(macros: &[MacroAction], state: &str) -> Vec<PriorAction> {
    macros
        .iter()
        .filter(|m| m.state == state)
        .map(|m| PriorAction {
            action: m.actions.join(" "),
            count: m.count,
            weighted_count: m.count as f64,
            success_rate: m.success_rate,
            mean_score: None,
            prior: m.prior,
            confidence: 0.0,
        })
        .collect()
}
