//! Experimental Shrink/Expand operation receipts and bounded state snapshots.
use serde::{Deserialize, Serialize};

/// Axis whose extent is locked by a spine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum ShrinkAxis {
    /// Locked width, producing a vertical spine.
    Horizontal,
    /// Locked height, producing a horizontal spine.
    Vertical,
}

/// Persistent layout form, independent of a temporary preview.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum ShrinkState {
    /// Ordinary native window allocation.
    #[default]
    Normal,
    /// Independent spine presentation.
    Spine,
}

/// Operation trigger, without layout or geometry history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum ShrinkExpandKind {
    /// Timed Shrink.
    ShrinkTimed(ShrinkAxis),
    /// Spatial mouse Shrink.
    ShrinkSpatial(ShrinkAxis),
    /// Timed selection among adjacent spines.
    ExpandSelection,
    /// Direct expansion by identity.
    ExpandDirect,
    /// Expansion of the workspace snapshot.
    ExpandAll,
}

/// Current operation phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum ShrinkExpandPhase {
    /// Temporary presentation, form unchanged.
    Previewing,
    /// Native form/allocation submitted; clients may still be pending.
    Submitted,
    /// Preview cancelled without submitting form.
    Cancelled,
    /// Native wait ended; inspect the reason.
    Settled,
}

/// Why a submitted operation stopped waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum ShrinkExpandFinishReason {
    /// Matching geometry or a processed client response.
    Ready,
    /// Original deadline expired; does not assert client completion.
    Deadline,
    /// All submitted targets disappeared.
    TargetsRemoved,
}

/// Lightweight retained operation record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct ShrinkExpandOperation {
    /// Independent operation identity.
    pub operation_id: u64,
    /// Increases on selection, submission and terminal changes, not animation frames.
    pub revision: u64,
    /// Native workspace identity at submission/start.
    pub workspace_id: u64,
    /// Actual window identities in this operation.
    pub targets: Vec<u64>,
    /// Trigger kind.
    pub kind: ShrinkExpandKind,
    /// Current phase.
    pub phase: ShrinkExpandPhase,
    /// Terminal submitted outcome, absent for previews/cancellation/pending submissions.
    pub reason: Option<ShrinkExpandFinishReason>,
}

/// Immediate command outcome, independent of a later client response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum ShrinkExpandOutcome {
    /// Entered or revised a preview.
    Previewing,
    /// Submitted form/allocation.
    Submitted,
    /// Cancelled a preview; did not execute the opposite operation.
    Cancelled,
    /// No target required a change.
    NoChange,
}

/// Receipt returned only after actual seat dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct ShrinkExpandActionReceipt {
    /// Absent when no operation was created.
    pub operation_id: Option<u64>,
    /// Actual operation workspace or current workspace for a no-op.
    pub workspace_id: Option<u64>,
    /// Actual operation target identities.
    pub targets: Vec<u64>,
    /// Immediate outcome.
    pub outcome: ShrinkExpandOutcome,
    /// Whether this submitted operation is currently waiting for clients.
    pub waiting_for_clients: bool,
}

/// Current preview, submitted batches and the most recent 64 terminal records.
/// Missing IDs are expired or unknown; a caller must not continue waiting for them.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct ShrinkExpandState {
    /// Complete replacement snapshot; contains no window size or layout history.
    pub operations: Vec<ShrinkExpandOperation>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{EventStreamState, EventStreamStatePart};
    use crate::{Event, Request, Response};

    #[test]
    fn shrink_snapshot_roundtrip_prefix_replaces_state_and_keeps_terminal_reason() {
        let snapshot = ShrinkExpandState {
            operations: vec![ShrinkExpandOperation {
                operation_id: 7,
                revision: 3,
                workspace_id: 2,
                targets: vec![11],
                kind: ShrinkExpandKind::ExpandDirect,
                phase: ShrinkExpandPhase::Settled,
                reason: Some(ShrinkExpandFinishReason::Deadline),
            }],
        };
        let json = serde_json::to_string(&Response::ShrinkExpandState(snapshot.clone())).unwrap();
        let Response::ShrinkExpandState(decoded) = serde_json::from_str(&json).unwrap() else {
            panic!()
        };
        assert_eq!(snapshot, decoded);
        let mut state = EventStreamState::default();
        assert!(state
            .apply(Event::ShrinkExpandChanged {
                state: snapshot.clone()
            })
            .is_none());
        let mut replica = EventStreamState::default();
        for event in state.replicate() {
            replica.apply(event);
        }
        assert_eq!(replica.shrink_expand.state, snapshot);
        state.apply(Event::ShrinkExpandChanged {
            state: Default::default(),
        });
        assert!(state.shrink_expand.state.operations.is_empty());
        assert!(matches!(
            serde_json::from_str::<Request>("\"ShrinkExpandState\"").unwrap(),
            Request::ShrinkExpandState
        ));
        assert!(serde_json::from_str::<Request>(r#"{"Action":{"ExpandWindow":{}}}"#).is_err());
    }
}
