//! Official, dependency-free domain adapters for converting domain records to
//! the generic `lineprior::Observation` model. These adapters do not parse
//! domain formats or validate actions; the owning application remains the
//! authority for that and can use the resulting prior as a non-oracular hint.

use lineprior::{Observation, Outcome};

fn observation(
    sequence_id: String,
    step: u32,
    state: String,
    action: String,
    outcome: Outcome,
    score: Option<f64>,
    source: &'static str,
) -> Observation {
    Observation {
        sequence_id,
        step,
        state,
        action,
        outcome,
        score,
        weight: 1.0,
        tags: vec![source.into()],
        observed_at_unix_seconds: None,
        source: Some(source.into()),
    }
}

/// Adapter for Sekirei's position/move vocabulary. SFEN/USI remain opaque.
pub mod sekirei {
    use super::*;
    pub struct Record {
        pub game_id: String,
        pub ply: u32,
        pub sfen: String,
        pub usi_move: String,
        pub outcome: Outcome,
        pub score: Option<f64>,
    }
    pub fn to_observation(record: Record) -> Observation {
        observation(
            record.game_id,
            record.ply,
            record.sfen,
            record.usi_move,
            record.outcome,
            record.score,
            "sekirei",
        )
    }
}

/// Adapter for UI automation traces. Screen states and actions are opaque IDs.
pub mod ui_automation {
    use super::*;
    pub struct Record {
        pub session_id: String,
        pub step: u32,
        pub screen_state: String,
        pub action: String,
        pub outcome: Outcome,
        pub score: Option<f64>,
    }
    pub fn to_observation(record: Record) -> Observation {
        observation(
            record.session_id,
            record.step,
            record.screen_state,
            record.action,
            record.outcome,
            record.score,
            "ui_automation",
        )
    }
}

/// Adapter for LLM-agent tool traces. Tool-call serialization is caller-owned.
pub mod llm_agent {
    use super::*;
    pub struct Record {
        pub task_id: String,
        pub step: u32,
        pub task_state: String,
        pub tool_call: String,
        pub outcome: Outcome,
        pub score: Option<f64>,
    }
    pub fn to_observation(record: Record) -> Observation {
        observation(
            record.task_id,
            record.step,
            record.task_state,
            record.tool_call,
            record.outcome,
            record.score,
            "llm_agent",
        )
    }
}

/// Adapter for retrosynthesis routes. Molecules/templates remain opaque IDs.
pub mod retrosynthesis {
    use super::*;
    pub struct Record {
        pub route_id: String,
        pub step: u32,
        pub intermediate: String,
        pub reaction_template: String,
        pub outcome: Outcome,
        pub score: Option<f64>,
    }
    pub fn to_observation(record: Record) -> Observation {
        observation(
            record.route_id,
            record.step,
            record.intermediate,
            record.reaction_template,
            record.outcome,
            record.score,
            "retrosynthesis",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_official_adapters_preserve_domain_values_and_add_source() {
        let a = sekirei::to_observation(sekirei::Record {
            game_id: "g".into(),
            ply: 3,
            sfen: "sfen".into(),
            usi_move: "7g7f".into(),
            outcome: Outcome::Success,
            score: Some(1.0),
        });
        let b = ui_automation::to_observation(ui_automation::Record {
            session_id: "u".into(),
            step: 1,
            screen_state: "screen".into(),
            action: "click:x".into(),
            outcome: Outcome::Unknown,
            score: None,
        });
        let c = llm_agent::to_observation(llm_agent::Record {
            task_id: "t".into(),
            step: 2,
            task_state: "state".into(),
            tool_call: "search".into(),
            outcome: Outcome::Failure,
            score: None,
        });
        let d = retrosynthesis::to_observation(retrosynthesis::Record {
            route_id: "r".into(),
            step: 0,
            intermediate: "mol".into(),
            reaction_template: "template".into(),
            outcome: Outcome::Draw,
            score: Some(0.2),
        });
        assert_eq!(
            (a.state, a.action, a.source),
            ("sfen".into(), "7g7f".into(), Some("sekirei".into()))
        );
        assert_eq!(b.tags, vec!["ui_automation"]);
        assert_eq!(c.source, Some("llm_agent".into()));
        assert_eq!(d.action, "template");
    }
}
