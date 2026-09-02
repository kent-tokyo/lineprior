use crate::{PriorAction, PriorBook, Result};
use std::collections::HashMap;

/// One independently-built book and its reliability multiplier.
#[derive(Debug, Clone)]
pub struct PriorBookSource<'a> {
    pub name: &'a str,
    pub weight: f64,
    pub book: &'a PriorBook,
}

type WeightedActions = Vec<(PriorAction, f64)>;

/// Merges independently-built books. Weights affect evidence and ranking;
/// source names remain caller provenance and never create actions.
pub fn merge_prior_books(sources: &[PriorBookSource<'_>]) -> Result<PriorBook> {
    if sources.is_empty() {
        return Err(crate::Error::InvalidConfig {
            message: "at least one prior book is required".into(),
        });
    }
    for source in sources {
        if source.name.is_empty() || !source.weight.is_finite() || source.weight < 0.0 {
            return Err(crate::Error::InvalidConfig {
                message: "source names must be non-empty and weights finite >= 0".into(),
            });
        }
    }
    Ok(PriorBook {
        entries: merge_entries(sources),
        context_entries: merge_context_entries(sources),
    })
}

fn merge_entries(sources: &[PriorBookSource<'_>]) -> HashMap<String, Vec<PriorAction>> {
    let mut groups: HashMap<String, Vec<(PriorAction, f64)>> = HashMap::new();
    for source in sources {
        for entry in source.book.entries_sorted() {
            groups
                .entry(entry.state)
                .or_default()
                .extend(entry.actions.into_iter().map(|a| (a, source.weight)));
        }
    }
    groups
        .into_iter()
        .map(|(state, actions)| (state, merge_actions(actions)))
        .collect()
}

fn merge_context_entries(
    sources: &[PriorBookSource<'_>],
) -> HashMap<(Vec<String>, String), Vec<PriorAction>> {
    let mut groups: HashMap<(Vec<String>, String), WeightedActions> = HashMap::new();
    for source in sources {
        for entry in source.book.context_entries_sorted() {
            groups
                .entry((entry.context, entry.state))
                .or_default()
                .extend(entry.actions.into_iter().map(|a| (a, source.weight)));
        }
    }
    groups
        .into_iter()
        .map(|(key, actions)| (key, merge_actions(actions)))
        .collect()
}

fn merge_actions(actions: Vec<(PriorAction, f64)>) -> Vec<PriorAction> {
    let mut by_action: HashMap<String, (u64, f64, f64, f64, f64)> = HashMap::new();
    for (action, source_weight) in actions {
        let entry = by_action.entry(action.action).or_default();
        entry.0 += action.count;
        entry.1 += source_weight * action.weighted_count;
        entry.2 += source_weight * action.success_rate.unwrap_or(0.0) * action.weighted_count;
        entry.3 += source_weight * action.weighted_count;
        entry.4 += source_weight * action.mean_score.unwrap_or(0.0) * action.weighted_count;
    }
    let mut output: Vec<PriorAction> = by_action
        .into_iter()
        .map(
            |(action, (count, weighted_count, successes, denominator, score_sum))| PriorAction {
                action,
                count,
                weighted_count,
                success_rate: (denominator > 0.0).then_some(successes / denominator),
                mean_score: (denominator > 0.0).then_some(score_sum / denominator),
                prior: 0.0,
                confidence: 0.0,
            },
        )
        .collect();
    let total = output
        .iter()
        .map(|action| action.weighted_count)
        .sum::<f64>();
    for action in &mut output {
        action.prior = if total > 0.0 {
            action.weighted_count / total
        } else {
            0.0
        };
    }
    output.sort_by(|left, right| {
        right
            .prior
            .total_cmp(&left.prior)
            .then(left.action.cmp(&right.action))
    });
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    fn book(action: &str, count: u64, prior: f64) -> PriorBook {
        PriorBook {
            entries: [(
                "s".into(),
                vec![PriorAction {
                    action: action.into(),
                    count,
                    weighted_count: count as f64,
                    success_rate: Some(1.0),
                    mean_score: Some(0.5),
                    prior,
                    confidence: 0.8,
                }],
            )]
            .into_iter()
            .collect(),
            context_entries: HashMap::new(),
        }
    }
    #[test]
    fn merges_books_with_weighted_evidence() {
        let first = book("a", 10, 1.0);
        let second = book("b", 10, 1.0);
        let merged = merge_prior_books(&[
            PriorBookSource {
                name: "first",
                weight: 2.0,
                book: &first,
            },
            PriorBookSource {
                name: "second",
                weight: 1.0,
                book: &second,
            },
        ])
        .unwrap();
        assert_eq!(merged.entries["s"][0].action, "a");
        assert!((merged.entries["s"][0].prior - 2.0 / 3.0).abs() < 1e-9);
    }
}
