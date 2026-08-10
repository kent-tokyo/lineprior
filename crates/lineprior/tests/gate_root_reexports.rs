//! Regression guard for issue #1: `GateStatus`/`PredictionStatus` must be
//! nameable and matchable from the crate root, not just reachable as a
//! struct field's type. An in-`src/` unit test wouldn't catch a missing
//! `pub use` (it can reach `gate::GateStatus` directly regardless), so this
//! lives as an integration test that only ever spells `lineprior::`.
//!
//! Also covers `ContextQueryResult`, found via the same failure mode while
//! fixing broken rustdoc intra-doc links: `PriorBook::query_with_context`
//! returns it, but it wasn't re-exported either.

use lineprior::{ContextQueryResult, GateStatus, PredictionStatus, PriorBook};

#[test]
fn gate_status_is_nameable_and_matchable_from_the_crate_root() {
    let status = GateStatus::Pass;
    let label = match status {
        GateStatus::Pass => "pass",
        GateStatus::Fail => "fail",
        GateStatus::Inconclusive => "inconclusive",
    };
    assert_eq!(label, "pass");
}

#[test]
fn prediction_status_is_nameable_and_matchable_from_the_crate_root() {
    let status = PredictionStatus::Supported;
    let label = match status {
        PredictionStatus::Supported => "supported",
        PredictionStatus::Extrapolation => "extrapolation",
        PredictionStatus::Unsupported => "unsupported",
    };
    assert_eq!(label, "supported");
}

#[test]
fn context_query_result_is_nameable_from_the_crate_root() {
    let book = PriorBook::default();
    let result: ContextQueryResult = book.query_with_context("state", &[], None);
    assert_eq!(result.matched_order, 0);
    assert!(result.candidates.is_empty());
}
