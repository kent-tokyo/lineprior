//! Regression guard for issue #1: `GateStatus`/`PredictionStatus` must be
//! nameable and matchable from the crate root, not just reachable as a
//! struct field's type. An in-`src/` unit test wouldn't catch a missing
//! `pub use` (it can reach `gate::GateStatus` directly regardless), so this
//! lives as an integration test that only ever spells `lineprior::`.

use lineprior::{GateStatus, PredictionStatus};

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
