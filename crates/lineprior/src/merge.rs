use crate::model::{PriorEntry, validate_prior_entry};
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
type MergedEntries = HashMap<String, Vec<PriorAction>>;
type MergedContextEntries = HashMap<(Vec<String>, String), Vec<PriorAction>>;

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
        entries: merge_entries(sources)?,
        context_entries: merge_context_entries(sources)?,
    })
}

fn merge_entries(sources: &[PriorBookSource<'_>]) -> Result<MergedEntries> {
    let mut groups: HashMap<String, Vec<(PriorAction, f64)>> = HashMap::new();
    for source in sources {
        for entry in source.book.entries_sorted() {
            validate_source_entry(source.name, &entry)?;
            groups
                .entry(entry.state)
                .or_default()
                .extend(entry.actions.into_iter().map(|a| (a, source.weight)));
        }
    }
    groups
        .into_iter()
        .map(|(state, actions)| Ok((state, merge_actions(actions)?)))
        .collect()
}

fn merge_context_entries(sources: &[PriorBookSource<'_>]) -> Result<MergedContextEntries> {
    let mut groups: HashMap<(Vec<String>, String), WeightedActions> = HashMap::new();
    for source in sources {
        for entry in source.book.context_entries_sorted() {
            validate_source_entry(source.name, &entry)?;
            groups
                .entry((entry.context, entry.state))
                .or_default()
                .extend(entry.actions.into_iter().map(|a| (a, source.weight)));
        }
    }
    groups
        .into_iter()
        .map(|(key, actions)| Ok((key, merge_actions(actions)?)))
        .collect()
}

fn validate_source_entry(source_name: &str, entry: &PriorEntry) -> Result<()> {
    validate_prior_entry(entry).map_err(|message| crate::Error::InvalidConfig {
        message: format!("source {source_name:?} contains an invalid prior entry: {message}"),
    })
}

fn merge_numeric_error() -> crate::Error {
    crate::Error::InvalidConfig {
        message: "merged weighted evidence is not finite".into(),
    }
}

fn merge_actions(actions: Vec<(PriorAction, f64)>) -> Result<Vec<PriorAction>> {
    let mut by_action: HashMap<String, (u64, f64, f64, f64, f64)> = HashMap::new();
    for (action, source_weight) in actions {
        let entry = by_action.entry(action.action).or_default();
        entry.0 = entry
            .0
            .checked_add(action.count)
            .ok_or_else(|| crate::Error::InvalidConfig {
                message: "merged action count exceeds u64".into(),
            })?;
        let weighted_count = source_weight * action.weighted_count;
        let successes = weighted_count * action.success_rate.unwrap_or(0.0);
        let score = weighted_count * action.mean_score.unwrap_or(0.0);
        let next = [
            entry.1 + weighted_count,
            entry.2 + successes,
            entry.3 + weighted_count,
            entry.4 + score,
        ];
        if !next.iter().all(|value| value.is_finite()) {
            return Err(merge_numeric_error());
        }
        entry.1 = next[0];
        entry.2 = next[1];
        entry.3 = next[2];
        entry.4 = next[3];
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
    let total = output.iter().try_fold(0.0, |total, action| {
        let next = total + action.weighted_count;
        next.is_finite()
            .then_some(next)
            .ok_or_else(merge_numeric_error)
    })?;
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
    Ok(output)
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

    #[test]
    fn merge_rejects_action_count_overflow() {
        let first = book("a", u64::MAX, 1.0);
        let second = book("a", 1, 1.0);
        let error = merge_prior_books(&[
            PriorBookSource {
                name: "first",
                weight: 1.0,
                book: &first,
            },
            PriorBookSource {
                name: "second",
                weight: 1.0,
                book: &second,
            },
        ])
        .unwrap_err();

        assert!(matches!(error, crate::Error::InvalidConfig { .. }));
        assert_eq!(
            error.to_string(),
            "invalid build config: merged action count exceeds u64"
        );
    }

    #[test]
    fn merge_rejects_invalid_source_values_and_float_overflow() {
        let mut invalid = book("a", 1, 1.0);
        invalid.entries.get_mut("s").unwrap()[0].confidence = f64::NAN;
        let error = merge_prior_books(&[PriorBookSource {
            name: "invalid",
            weight: 1.0,
            book: &invalid,
        }])
        .unwrap_err();
        assert!(error.to_string().contains("invalid prior entry"));

        let mut overflowing = book("a", 1, 1.0);
        overflowing.entries.get_mut("s").unwrap()[0].weighted_count = f64::MAX;
        let error = merge_prior_books(&[PriorBookSource {
            name: "overflowing",
            weight: 2.0,
            book: &overflowing,
        }])
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid build config: merged weighted evidence is not finite"
        );
    }
}
