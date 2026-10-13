//! Seat operation coordination. Records contain identities/results, never layout history.
mod runtime;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub use runtime::Rejected;
pub(crate) use runtime::{
    ExpandFocusLease, ExpandSelectionLease, PreviewLease, SpatialInput, SpatialSession, SpineClick,
};

use crate::layout::shrink_expand::ShrinkAxis;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    ShrinkTimed(ShrinkAxis),
    ShrinkSpatial(ShrinkAxis),
    ExpandSelection,
    ExpandDirect,
    ExpandAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Previewing,
    Submitted,
    Cancelled,
    Settled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishReason {
    Ready,
    Deadline,
    TargetsRemoved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub id: u64,
    pub revision: u64,
    pub workspace: u64,
    pub targets: Vec<u64>,
    pub kind: Kind,
    pub phase: Phase,
    pub reason: Option<FinishReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket {
    pub id: u64,
    pub revision: u64,
}

impl Record {
    pub fn ticket(&self) -> Ticket {
        Ticket {
            id: self.id,
            revision: self.revision,
        }
    }
}

impl From<ShrinkAxis> for niri_ipc::ShrinkAxis {
    fn from(axis: ShrinkAxis) -> Self {
        match axis {
            ShrinkAxis::Horizontal => Self::Horizontal,
            ShrinkAxis::Vertical => Self::Vertical,
        }
    }
}

impl From<&Record> for niri_ipc::ShrinkExpandOperation {
    fn from(record: &Record) -> Self {
        use niri_ipc::{
            ShrinkExpandFinishReason as Reason, ShrinkExpandKind as IpcKind,
            ShrinkExpandPhase as IpcPhase,
        };
        Self {
            operation_id: record.id,
            revision: record.revision,
            workspace_id: record.workspace,
            targets: record.targets.clone(),
            kind: match record.kind {
                Kind::ShrinkTimed(axis) => IpcKind::ShrinkTimed(axis.into()),
                Kind::ShrinkSpatial(axis) => IpcKind::ShrinkSpatial(axis.into()),
                Kind::ExpandSelection => IpcKind::ExpandSelection,
                Kind::ExpandDirect => IpcKind::ExpandDirect,
                Kind::ExpandAll => IpcKind::ExpandAll,
            },
            phase: match record.phase {
                Phase::Previewing => IpcPhase::Previewing,
                Phase::Submitted => IpcPhase::Submitted,
                Phase::Cancelled => IpcPhase::Cancelled,
                Phase::Settled => IpcPhase::Settled,
            },
            reason: record.reason.map(|r| match r {
                FinishReason::Ready => Reason::Ready,
                FinishReason::Deadline => Reason::Deadline,
                FinishReason::TargetsRemoved => Reason::TargetsRemoved,
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Preview {
    pub record: Record,
    pub deadline: Option<Instant>,
}

#[derive(Debug)]
struct Submitted {
    record: Record,
    deadline: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetState {
    Removed,
    Waiting,
    Ready,
    Deadline,
}

#[derive(Debug, Default)]
pub struct Coordinator {
    next_id: u64,
    preview: Option<Preview>,
    submitted: Vec<Submitted>,
    recent: VecDeque<Record>,
}

impl Coordinator {
    pub fn ipc_state(&self) -> niri_ipc::ShrinkExpandState {
        niri_ipc::ShrinkExpandState {
            operations: self.records().map(Into::into).collect(),
        }
    }
    pub fn preview(&self) -> Option<&Preview> {
        self.preview.as_ref()
    }

    pub fn records(&self) -> impl Iterator<Item = &Record> {
        self.preview
            .iter()
            .map(|p| &p.record)
            .chain(self.submitted.iter().map(|s| &s.record))
            .chain(self.recent.iter())
    }

    pub fn start(
        &mut self,
        workspace: u64,
        targets: Vec<u64>,
        kind: Kind,
        now: Instant,
        delay: Duration,
    ) -> Record {
        assert!(!targets.is_empty());
        self.cancel();
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("operation IDs exhausted");
        let record = Record {
            id: self.next_id,
            revision: 0,
            workspace,
            targets,
            kind,
            phase: Phase::Previewing,
            reason: None,
        };
        self.preview = Some(Preview {
            record: record.clone(),
            deadline: deadline(kind, now, delay),
        });
        record
    }

    pub fn revise(
        &mut self,
        kind: Kind,
        targets: Vec<u64>,
        now: Instant,
        delay: Duration,
    ) -> Option<Record> {
        assert!(!targets.is_empty());
        let preview = self.preview.as_mut()?;
        preview.record.revision += 1;
        preview.record.kind = kind;
        preview.record.targets = targets;
        preview.deadline = deadline(kind, now, delay);
        Some(preview.record.clone())
    }

    pub fn matches(&self, ticket: Ticket) -> bool {
        self.preview
            .as_ref()
            .is_some_and(|p| p.record.ticket() == ticket)
    }

    pub fn timer_due(&self, ticket: Ticket, now: Instant) -> bool {
        self.preview
            .as_ref()
            .is_some_and(|p| p.record.ticket() == ticket && p.deadline.is_some_and(|at| now >= at))
    }

    pub fn cancel(&mut self) -> Option<Record> {
        let mut record = self.preview.take()?.record;
        record.revision += 1;
        record.phase = Phase::Cancelled;
        self.remember(record.clone());
        Some(record)
    }

    /// Called only after the caller successfully changes form and submits native allocation.
    pub fn submit(
        &mut self,
        ticket: Ticket,
        actual_targets: Vec<u64>,
        deadline: Option<Instant>,
    ) -> Option<Record> {
        if !self.matches(ticket) || actual_targets.is_empty() {
            return None;
        }
        let mut record = self.preview.take().unwrap().record;
        record.revision += 1;
        record.targets = actual_targets;
        record.phase = Phase::Submitted;
        self.submitted.push(Submitted {
            record: record.clone(),
            deadline,
        });
        Some(record)
    }

    /// Direct expansion has no selection preview. Called after native form/allocation submission.
    pub fn submit_immediate(
        &mut self,
        workspace: u64,
        targets: Vec<u64>,
        kind: Kind,
        deadline: Option<Instant>,
    ) -> Record {
        assert!(self.preview.is_none());
        assert!(!targets.is_empty());
        assert!(matches!(kind, Kind::ExpandDirect | Kind::ExpandAll));
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("operation IDs exhausted");
        let record = Record {
            id: self.next_id,
            revision: 0,
            workspace,
            targets,
            kind,
            phase: Phase::Submitted,
            reason: None,
        };
        self.submitted.push(Submitted {
            record: record.clone(),
            deadline,
        });
        record
    }

    pub fn reconcile(&mut self, now: Instant, mut target_state: impl FnMut(u64) -> TargetState) {
        let mut pending = Vec::new();
        for mut submitted in std::mem::take(&mut self.submitted) {
            let mut alive = false;
            let mut waiting = false;
            let mut degraded = false;
            for id in &submitted.record.targets {
                match target_state(*id) {
                    TargetState::Removed => (),
                    TargetState::Waiting => {
                        alive = true;
                        waiting = true;
                    }
                    TargetState::Ready => alive = true,
                    TargetState::Deadline => {
                        alive = true;
                        degraded |= submitted.deadline.is_some();
                    }
                }
            }
            let reason = if !alive {
                Some(FinishReason::TargetsRemoved)
            } else if !waiting {
                Some(if degraded {
                    FinishReason::Deadline
                } else {
                    FinishReason::Ready
                })
            } else if submitted.deadline.is_some_and(|at| now >= at) {
                Some(FinishReason::Deadline)
            } else {
                None
            };
            if let Some(reason) = reason {
                submitted.record.revision += 1;
                submitted.record.phase = Phase::Settled;
                submitted.record.reason = Some(reason);
                self.remember(submitted.record);
            } else {
                pending.push(submitted);
            }
        }
        self.submitted = pending;
    }

    fn remember(&mut self, record: Record) {
        self.recent.push_back(record);
        if self.recent.len() > 64 {
            self.recent.pop_front();
        }
    }
}

fn deadline(kind: Kind, now: Instant, delay: Duration) -> Option<Instant> {
    match kind {
        Kind::ShrinkSpatial(_) | Kind::ExpandDirect | Kind::ExpandAll => None,
        _ => Some(now + delay),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_preserves_identity_and_invalidates_old_callback_without_early_commit() {
        let mut c = Coordinator::default();
        let now = Instant::now();
        let first = c.start(
            7,
            vec![11],
            Kind::ShrinkTimed(ShrinkAxis::Horizontal),
            now,
            Duration::from_millis(700),
        );
        let repeated = c
            .revise(
                Kind::ShrinkTimed(ShrinkAxis::Vertical),
                vec![11],
                now + Duration::from_millis(600),
                Duration::from_millis(700),
            )
            .unwrap();
        assert_eq!(first.id, repeated.id);
        assert!(!c.timer_due(first.ticket(), now + Duration::from_secs(5)));
        assert!(!c.timer_due(repeated.ticket(), now + Duration::from_millis(700)));
        assert!(c.timer_due(repeated.ticket(), now + Duration::from_millis(1300)));
        assert!(c.submit(first.ticket(), vec![11], None).is_none());
        assert!(c.matches(repeated.ticket()));
        c.cancel();
        assert!(!c.matches(repeated.ticket()));
        let next = c.start(7, vec![11], repeated.kind, now, Duration::ZERO);
        assert_ne!(next.id, repeated.id);
    }

    #[test]
    fn spatial_preview_has_no_timer_and_cancel_never_undoes_a_submission() {
        let mut c = Coordinator::default();
        let now = Instant::now();
        let r = c.start(
            1,
            vec![2],
            Kind::ShrinkSpatial(ShrinkAxis::Horizontal),
            now,
            Duration::ZERO,
        );
        assert!(!c.timer_due(r.ticket(), now + Duration::from_secs(100)));
        assert!(c.preview().unwrap().deadline.is_none());
        c.submit(r.ticket(), vec![2], None).unwrap();
        assert!(c.cancel().is_none());
        c.reconcile(now, |_| TargetState::Ready);
        assert_eq!(
            c.records().next().unwrap().reason,
            Some(FinishReason::Ready)
        );
    }

    #[test]
    fn submitted_batches_keep_their_deadline_and_survive_new_previews() {
        let mut c = Coordinator::default();
        let now = Instant::now();
        let r = c.start(1, vec![2, 3], Kind::ExpandSelection, now, Duration::ZERO);
        c.submit(
            r.ticket(),
            vec![2, 3],
            Some(now + Duration::from_millis(300)),
        )
        .unwrap();
        let old_id = r.id;
        let new = c.start(
            1,
            vec![4],
            Kind::ShrinkTimed(ShrinkAxis::Horizontal),
            now,
            Duration::from_secs(1),
        );
        c.reconcile(now + Duration::from_millis(299), |id| {
            if id == 2 {
                TargetState::Removed
            } else {
                TargetState::Waiting
            }
        });
        assert_eq!(
            c.records().filter(|r| r.phase == Phase::Submitted).count(),
            1
        );
        c.reconcile(now + Duration::from_millis(300), |_| TargetState::Waiting);
        assert!(c.matches(new.ticket()));
        assert!(c
            .records()
            .any(|r| r.id == old_id && r.reason == Some(FinishReason::Deadline)));
        // A late ready observation cannot rewrite the retained deadline terminal result.
        c.reconcile(now + Duration::from_secs(1), |_| TargetState::Ready);
        assert!(c
            .records()
            .any(|r| r.reason == Some(FinishReason::Deadline)));
    }

    #[test]
    fn removed_targets_settle_and_terminal_retention_is_bounded() {
        let mut c = Coordinator::default();
        let now = Instant::now();
        let r = c.start(1, vec![2], Kind::ExpandSelection, now, Duration::ZERO);
        c.submit(r.ticket(), vec![2], Some(now + Duration::from_secs(1)))
            .unwrap();
        c.reconcile(now, |_| TargetState::Removed);
        assert_eq!(
            c.records().next().unwrap().reason,
            Some(FinishReason::TargetsRemoved)
        );
        for _ in 0..64 {
            c.start(1, vec![2], Kind::ExpandSelection, now, Duration::ZERO);
            c.cancel();
        }
        assert_eq!(c.records().count(), 64);
        assert!(!c.records().any(|record| record.id == r.id));
    }
}
