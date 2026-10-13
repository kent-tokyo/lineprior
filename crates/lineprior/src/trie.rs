use crate::model::sort_actions;
use crate::{ContextQueryResult, PriorAction, PriorBook};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Default)]
struct TrieNode {
    children: BTreeMap<String, TrieNode>,
    states: BTreeMap<String, Vec<PriorAction>>,
}

/// Deterministic prefix-tree representation of context priors.
#[derive(Debug, Clone, Default)]
pub struct PriorTrie {
    order_zero: HashMap<String, Vec<PriorAction>>,
    root: TrieNode,
}

impl PriorTrie {
    /// Builds a trie from an existing book without changing the book itself.
    pub fn from_book(book: &PriorBook) -> Self {
        let mut order_zero = book.entries.clone();
        for actions in order_zero.values_mut() {
            sort_actions(actions);
        }
        let mut trie = Self {
            order_zero,
            root: TrieNode::default(),
        };
        for ((context, state), actions) in &book.context_entries {
            let mut node = &mut trie.root;
            for part in context {
                node = node.children.entry(part.clone()).or_default();
            }
            let mut ranked = actions.clone();
            sort_actions(&mut ranked);
            node.states.insert(state.clone(), ranked);
        }
        trie
    }

    /// Queries the longest available context suffix, falling back to order 0.
    pub fn query(
        &self,
        state: &str,
        recent_actions: &[String],
        top_k: Option<usize>,
    ) -> ContextQueryResult {
        for start in 0..recent_actions.len() {
            let mut node = &self.root;
            let mut found = true;
            for part in &recent_actions[start..] {
                let Some(next) = node.children.get(part) else {
                    found = false;
                    break;
                };
                node = next;
            }
            if found && let Some(actions) = node.states.get(state) {
                let len = top_k.unwrap_or(actions.len()).min(actions.len());
                let candidates = actions[..len].to_vec();
                return ContextQueryResult {
                    matched_order: recent_actions.len() - start,
                    candidates,
                };
            }
        }
        let candidates = self.order_zero.get(state).map_or_else(Vec::new, |actions| {
            let len = top_k.unwrap_or(actions.len()).min(actions.len());
            actions[..len].to_vec()
        });
        ContextQueryResult {
            matched_order: 0,
            candidates,
        }
    }
}

impl PriorBook {
    /// Materializes context entries into a deterministic trie for repeated
    /// queries. The flat book remains the canonical serialization format.
    pub fn to_trie(&self) -> PriorTrie {
        PriorTrie::from_book(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trie_prefers_longest_suffix_then_order_zero() {
        let mut book = PriorBook::default();
        book.entries.insert(
            "s".into(),
            vec![PriorAction {
                action: "base".into(),
                count: 1,
                weighted_count: 1.0,
                success_rate: None,
                mean_score: None,
                prior: 1.0,
                confidence: 0.1,
            }],
        );
        book.context_entries.insert(
            (vec!["a".into()], "s".into()),
            vec![PriorAction {
                action: "context".into(),
                count: 1,
                weighted_count: 1.0,
                success_rate: None,
                mean_score: None,
                prior: 1.0,
                confidence: 0.1,
            }],
        );
        let trie = book.to_trie();
        let result = trie.query("s", &["a".into()], None);
        assert_eq!(result.matched_order, 1);
        assert_eq!(result.candidates[0].action, "context");
        assert_eq!(
            trie.query("s", &["x".into()], None).candidates[0].action,
            "base"
        );
    }

    #[test]
    fn trie_ranks_ad_hoc_unsorted_entries_before_top_k_truncation() {
        fn action(name: &str, prior: f64) -> PriorAction {
            PriorAction {
                action: name.into(),
                count: 1,
                weighted_count: 1.0,
                success_rate: None,
                mean_score: None,
                prior,
                confidence: 0.1,
            }
        }

        let mut book = PriorBook::default();
        book.entries
            .insert("s".into(), vec![action("low", 0.1), action("best", 0.9)]);
        book.context_entries.insert(
            (vec!["a".into()], "s".into()),
            vec![action("z", 0.5), action("a", 0.5)],
        );
        let trie = book.to_trie();

        assert_eq!(trie.query("s", &[], Some(1)).candidates[0].action, "best");
        assert_eq!(
            trie.query("s", &["a".into()], Some(1)).candidates[0].action,
            "a"
        );
    }
}
