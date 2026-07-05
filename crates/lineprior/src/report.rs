use crate::model::PriorBook;
use crate::score::entropy_bits;
use serde::Serialize;

/// Entropy of one state's prior distribution, in bits. Low entropy means
/// one action dominates and the prior is likely useful; high entropy
/// means many actions compete and a fallback search may be safer.
#[derive(Debug, Clone, Serialize)]
pub struct StateEntropy {
    pub state: String,
    pub entropy_bits: f64,
    pub num_actions: usize,
}

/// Aggregate statistics over an entire prior book, for the CLI `summary` command.
#[derive(Debug, Clone, Serialize)]
pub struct SummaryReport {
    pub num_states: usize,
    pub num_action_entries: usize,
    pub avg_confidence: f64,
    pub avg_entropy_bits: f64,
}

/// Per-state entropy, sorted lexicographically by state for determinism.
pub fn state_entropy(book: &PriorBook) -> Vec<StateEntropy> {
    let mut result: Vec<StateEntropy> = book
        .entries
        .iter()
        .map(|(state, actions)| {
            let priors: Vec<f64> = actions.iter().map(|a| a.prior).collect();
            StateEntropy {
                state: state.clone(),
                entropy_bits: entropy_bits(&priors),
                num_actions: actions.len(),
            }
        })
        .collect();
    result.sort_by(|a, b| a.state.cmp(&b.state));
    result
}

/// Summarizes a whole prior book: how many states/actions it covers and
/// how confident/decisive it tends to be.
pub fn summarize(book: &PriorBook) -> SummaryReport {
    let num_states = book.entries.len();
    let num_action_entries: usize = book.entries.values().map(Vec::len).sum();

    if num_action_entries == 0 {
        return SummaryReport {
            num_states,
            num_action_entries: 0,
            avg_confidence: 0.0,
            avg_entropy_bits: 0.0,
        };
    }

    let total_confidence: f64 = book
        .entries
        .values()
        .flat_map(|actions| actions.iter())
        .map(|a| a.confidence)
        .sum();
    let entropies = state_entropy(book);
    let total_entropy: f64 = entropies.iter().map(|e| e.entropy_bits).sum();

    SummaryReport {
        num_states,
        num_action_entries,
        avg_confidence: total_confidence / num_action_entries as f64,
        avg_entropy_bits: total_entropy / num_states.max(1) as f64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PriorAction;
    use std::collections::HashMap;

    fn action(name: &str, prior: f64, confidence: f64) -> PriorAction {
        PriorAction {
            action: name.to_string(),
            count: 10,
            weighted_count: 10.0,
            success_rate: None,
            mean_score: None,
            prior,
            confidence,
        }
    }

    #[test]
    fn low_entropy_when_one_action_dominates() {
        let mut entries = HashMap::new();
        entries.insert(
            "s".to_string(),
            vec![action("a", 0.99, 0.9), action("b", 0.01, 0.9)],
        );
        let book = PriorBook { entries };
        let entropy = state_entropy(&book);
        assert!(entropy[0].entropy_bits < 0.2);
    }

    #[test]
    fn high_entropy_when_actions_compete_evenly() {
        let mut entries = HashMap::new();
        entries.insert(
            "s".to_string(),
            vec![
                action("a", 0.25, 0.9),
                action("b", 0.25, 0.9),
                action("c", 0.25, 0.9),
                action("d", 0.25, 0.9),
            ],
        );
        let book = PriorBook { entries };
        let entropy = state_entropy(&book);
        assert!((entropy[0].entropy_bits - 2.0).abs() < 1e-9);
    }

    #[test]
    fn summarize_averages_across_the_whole_book() {
        let mut entries = HashMap::new();
        entries.insert("s1".to_string(), vec![action("a", 1.0, 0.5)]);
        entries.insert("s2".to_string(), vec![action("b", 1.0, 1.0)]);
        let book = PriorBook { entries };
        let summary = summarize(&book);
        assert_eq!(summary.num_states, 2);
        assert_eq!(summary.num_action_entries, 2);
        assert!((summary.avg_confidence - 0.75).abs() < 1e-9);
    }
}
