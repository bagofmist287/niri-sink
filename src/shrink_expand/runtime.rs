use std::time::{Duration, Instant};

use calloop::timer::{TimeoutAction, Timer};
use smithay::output::Output;

use super::{Kind, Record, TargetState, Ticket};
use crate::layout::shrink_expand::ShrinkAxis;
use crate::layout::LayoutElement as _;
use crate::niri::{KeyboardFocus, State};

#[derive(Debug, Clone)]
pub(crate) struct PreviewLease {
    output: Output,
    thickness: f64,
    border: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    Disabled,
    Blocked,
    NoTarget,
    NoLegalAxis,
}

impl State {
    fn can_start_shrink_preview(&self) -> bool {
        self.can_recall_sink(false)
            && matches!(self.niri.keyboard_focus, KeyboardFocus::Layout { .. })
            && !self.niri.screenshot_ui.is_open()
            && !self.niri.exit_confirm_dialog.is_open()
    }

    /// Internal timed entry. Public actions are added only with their presentation/IPC adapter.
    #[doc(hidden)]
    pub fn begin_shrink_preview(&mut self, axis: Option<ShrinkAxis>) -> Result<Record, Rejected> {
        if !self.niri.config.borrow().shrink_expand.enabled {
            return Err(Rejected::Disabled);
        }
        if let Some(preview) = self.niri.shrink_expand.preview() {
            if !matches!(preview.record.kind, Kind::ShrinkTimed(_)) || !self.shrink_preview_valid()
            {
                return Ok(self.cancel_shrink_expand_preview().unwrap());
            }
        }
        if !self.can_start_shrink_preview() {
            return Err(Rejected::Blocked);
        }
        let target = self.niri.layout.focus().ok_or(Rejected::NoTarget)?;
        let target_id = target.id().get();
        let window = target.window.clone();
        let behavior = self.niri.config.borrow().shrink_expand;
        let plans = self
            .niri
            .layout
            .shrink_plans(&window, behavior.thickness)
            .ok_or(Rejected::NoLegalAxis)?;
        let chosen = if let Some(axis) = axis {
            plans.get(axis).fits.then_some(plans.get(axis))
        } else if let Some(preview) = self.niri.shrink_expand.preview() {
            let Kind::ShrinkTimed(current) = preview.record.kind else {
                unreachable!()
            };
            plans.next(current)
        } else {
            plans.first()
        }
        .ok_or(Rejected::NoLegalAxis)?;
        let workspace = self
            .niri
            .layout
            .active_workspace()
            .ok_or(Rejected::NoTarget)?;
        let border = workspace
            .tiles()
            .find(|tile| tile.window().window == window)
            .ok_or(Rejected::NoTarget)?
            .effective_border_width();
        let workspace_id = workspace.id().get();
        let output = self
            .niri
            .layout
            .active_output()
            .ok_or(Rejected::NoTarget)?
            .clone();
        let now = Instant::now();
        let delay = Duration::from_millis(u64::from(behavior.shrink_buffer_ms));
        let kind = Kind::ShrinkTimed(chosen.geometry.axis);
        let record = if self.niri.shrink_expand.preview().is_some() {
            self.niri
                .shrink_expand
                .revise(kind, vec![target_id], now, delay)
                .unwrap()
        } else {
            self.niri
                .shrink_expand
                .start(workspace_id, vec![target_id], kind, now, delay)
        };
        self.niri.shrink_preview_lease = Some(PreviewLease {
            output,
            thickness: behavior.thickness,
            border,
        });
        self.arm_shrink_preview_timer();
        self.niri.queue_redraw_all();
        Ok(record)
    }

    fn shrink_preview_valid(&self) -> bool {
        let Some(preview) = self.niri.shrink_expand.preview() else {
            return false;
        };
        let Some(lease) = &self.niri.shrink_preview_lease else {
            return false;
        };
        let Kind::ShrinkTimed(axis) = preview.record.kind else {
            return false;
        };
        if !self.niri.config.borrow().shrink_expand.enabled
            || !self.can_start_shrink_preview()
            || self.niri.config.borrow().shrink_expand.thickness != lease.thickness
            || self.niri.layout.active_output() != Some(&lease.output)
        {
            return false;
        }
        let Some(workspace) = self.niri.layout.active_workspace() else {
            return false;
        };
        if workspace.id().get() != preview.record.workspace {
            return false;
        }
        let Some(target) = self.niri.layout.focus() else {
            return false;
        };
        if preview.record.targets.as_slice() != [target.id().get()] {
            return false;
        }
        let Some(tile) = workspace
            .tiles()
            .find(|tile| tile.window().window == target.window)
        else {
            return false;
        };
        tile.effective_border_width() == lease.border
            && self
                .niri
                .layout
                .shrink_plans(&target.window, lease.thickness)
                .is_some_and(|plans| plans.get(axis).fits)
    }

    pub(crate) fn cancel_shrink_expand_preview(&mut self) -> Option<Record> {
        if let Some((token, _)) = self.niri.shrink_preview_timer.take() {
            self.niri.event_loop.remove(token);
        }
        self.niri.shrink_preview_lease = None;
        let record = self.niri.shrink_expand.cancel();
        if record.is_some() {
            self.niri.queue_redraw_all();
        }
        record
    }

    #[doc(hidden)]
    pub fn confirm_shrink_preview(&mut self, ticket: Ticket) -> Option<Record> {
        if !self.niri.shrink_expand.matches(ticket) {
            return None;
        }
        if !self.shrink_preview_valid() {
            return self.cancel_shrink_expand_preview();
        }
        let preview = self.niri.shrink_expand.preview().unwrap().clone();
        let Kind::ShrinkTimed(axis) = preview.record.kind else {
            return None;
        };
        let target = self.niri.layout.focus().unwrap().window.clone();
        let thickness = self.niri.shrink_preview_lease.as_ref().unwrap().thickness;
        if !self.niri.layout.commit_shrink(&target, axis, thickness) {
            return self.cancel_shrink_expand_preview();
        }
        if let Some((token, _)) = self.niri.shrink_preview_timer.take() {
            self.niri.event_loop.remove(token);
        }
        self.niri.shrink_preview_lease = None;
        let record = self
            .niri
            .shrink_expand
            .submit(ticket, preview.record.targets, None)
            .unwrap();
        self.refresh_shrink_expand_operations();
        self.niri.queue_redraw_all();
        Some(record)
    }

    fn arm_shrink_preview_timer(&mut self) {
        if let Some((token, _)) = self.niri.shrink_preview_timer.take() {
            self.niri.event_loop.remove(token);
        }
        let preview = self.niri.shrink_expand.preview().unwrap();
        let Some(deadline) = preview.deadline else {
            return;
        };
        let ticket = preview.record.ticket();
        let token = self
            .niri
            .event_loop
            .insert_source(Timer::from_deadline(deadline), move |_, _, state| {
                // Removing a timer cannot make a previously queued callback authoritative again.
                if state
                    .niri
                    .shrink_preview_timer
                    .as_ref()
                    .is_some_and(|(_, current)| *current == ticket)
                {
                    state.niri.shrink_preview_timer = None;
                }
                if state.niri.shrink_expand.timer_due(ticket, Instant::now()) {
                    state.confirm_shrink_preview(ticket);
                }
                TimeoutAction::Drop
            })
            .unwrap();
        self.niri.shrink_preview_timer = Some((token, ticket));
    }

    pub(crate) fn refresh_shrink_expand_operations(&mut self) {
        if self.niri.shrink_expand.preview().is_some() && !self.shrink_preview_valid() {
            self.cancel_shrink_expand_preview();
        }
        let layout = &self.niri.layout;
        self.niri.shrink_expand.reconcile(Instant::now(), |id| {
            let Some((_, window)) = layout.windows().find(|(_, window)| window.id().get() == id)
            else {
                return TargetState::Removed;
            };
            if window.normal_restore_deadline().is_some() {
                TargetState::Waiting
            } else {
                TargetState::Ready
            }
        });
    }
}
