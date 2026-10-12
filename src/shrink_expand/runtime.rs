use std::time::{Duration, Instant};

use calloop::timer::{TimeoutAction, Timer};
use smithay::desktop::Window;
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

#[derive(Debug, Clone)]
pub(crate) struct ExpandFocusLease {
    window: Window,
    workspace: u64,
    output: Option<Output>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    Disabled,
    Blocked,
    NoTarget,
    NoLegalAxis,
}

impl State {
    /// Used before common action side effects by both IPC and native bindings.
    pub(crate) fn dispatch_shrink_expand_action(
        &mut self,
        action: &niri_config::Action,
    ) -> Option<Result<niri_ipc::ShrinkExpandActionReceipt, String>> {
        use super::Phase;
        use niri_config::Action;
        use niri_ipc::{ShrinkExpandActionReceipt as Receipt, ShrinkExpandOutcome as Outcome};
        if !matches!(
            action,
            Action::ShrinkWindow
                | Action::ExpandWindow(_)
                | Action::ExpandAllWindows
                | Action::ConfirmShrinkExpand
                | Action::CancelShrinkExpand
        ) {
            return None;
        }
        if self.niri.is_locked() {
            return Some(Err("Shrink/Expand is blocked while locked".into()));
        }
        if !self.niri.config.borrow().shrink_expand.enabled
            && !matches!(action, Action::CancelShrinkExpand)
        {
            return Some(Err("Shrink/Expand is disabled".into()));
        }
        let result = match action {
            Action::ShrinkWindow => self.begin_shrink_preview(None).map(Some),
            Action::ExpandWindow(id) => self.expand_spine_window(*id),
            Action::ExpandAllWindows => self.expand_all_spines(),
            Action::ConfirmShrinkExpand => {
                let ticket = self.niri.shrink_expand.preview().map(|p| p.record.ticket());
                Ok(ticket.and_then(|ticket| self.confirm_shrink_preview(ticket)))
            }
            Action::CancelShrinkExpand => Ok(self.cancel_shrink_expand_preview()),
            _ => unreachable!(),
        };
        Some(
            result
                .map(|record| {
                    if let Some(record) = record {
                        Receipt {
                            operation_id: Some(record.id),
                            workspace_id: Some(record.workspace),
                            targets: record.targets,
                            outcome: match record.phase {
                                Phase::Previewing => Outcome::Previewing,
                                Phase::Cancelled => Outcome::Cancelled,
                                Phase::Submitted | Phase::Settled => Outcome::Submitted,
                            },
                            waiting_for_clients: self
                                .niri
                                .shrink_expand
                                .records()
                                .any(|r| r.id == record.id && r.phase == Phase::Submitted),
                        }
                    } else {
                        Receipt {
                            operation_id: None,
                            workspace_id: self
                                .niri
                                .layout
                                .active_workspace()
                                .map(|ws| ws.id().get()),
                            targets: Vec::new(),
                            outcome: Outcome::NoChange,
                            waiting_for_clients: false,
                        }
                    }
                })
                .map_err(|reason| match reason {
                    Rejected::Disabled => "Shrink/Expand is disabled".into(),
                    Rejected::Blocked => {
                        "Shrink/Expand is blocked by current seat ownership".into()
                    }
                    Rejected::NoTarget => "Shrink/Expand target is unavailable".into(),
                    Rejected::NoLegalAxis => "No legal Shrink axis is available".into(),
                }),
        )
    }

    fn can_expand_spines(&self) -> bool {
        self.can_recall_sink(false)
            && !self.niri.screenshot_ui.is_open()
            && !self.niri.exit_confirm_dialog.is_open()
    }

    /// Internal direct entry, sharing the current-slot allocator and protocol barrier.
    #[doc(hidden)]
    pub fn expand_spine_window(&mut self, id: u64) -> Result<Option<Record>, Rejected> {
        if !self.niri.config.borrow().shrink_expand.enabled {
            return Err(Rejected::Disabled);
        }
        if self.niri.shrink_expand.preview().is_some() {
            return Ok(self.cancel_shrink_expand_preview());
        }
        if !self.can_expand_spines() {
            return Err(Rejected::Blocked);
        }
        let (_, target) = self
            .niri
            .layout
            .windows()
            .find(|(_, window)| window.id().get() == id)
            .ok_or(Rejected::NoTarget)?;
        let window = target.window.clone();
        let workspace = self
            .niri
            .layout
            .workspaces()
            .find(|(_, _, ws)| ws.has_window(&window))
            .ok_or(Rejected::NoTarget)?
            .2
            .id()
            .get();
        self.submit_spine_expansion(
            workspace,
            vec![window.clone()],
            Kind::ExpandDirect,
            Some(window),
        )
    }

    /// One workspace snapshot; includes hidden tabs and floating spines without activating them.
    #[doc(hidden)]
    pub fn expand_all_spines(&mut self) -> Result<Option<Record>, Rejected> {
        if !self.niri.config.borrow().shrink_expand.enabled {
            return Err(Rejected::Disabled);
        }
        if self.niri.shrink_expand.preview().is_some() {
            return Ok(self.cancel_shrink_expand_preview());
        }
        if !self.can_expand_spines() {
            return Err(Rejected::Blocked);
        }
        let workspace = self
            .niri
            .layout
            .active_workspace()
            .ok_or(Rejected::NoTarget)?;
        let id = workspace.id().get();
        let windows = workspace
            .tiles()
            .filter(|tile| tile.is_spine())
            .map(|tile| tile.window().window.clone())
            .collect::<Vec<_>>();
        self.submit_spine_expansion(id, windows, Kind::ExpandAll, None)
    }

    fn submit_spine_expansion(
        &mut self,
        workspace: u64,
        windows: Vec<Window>,
        kind: Kind,
        focus: Option<Window>,
    ) -> Result<Option<Record>, Rejected> {
        let actual = self
            .niri
            .layout
            .workspaces_mut()
            .find(|ws| ws.id().get() == workspace)
            .ok_or(Rejected::NoTarget)?
            .restore_normal_spines(&windows);
        if actual.is_empty() {
            return Ok(None);
        }
        let targets = self
            .niri
            .layout
            .windows()
            .filter(|(_, w)| actual.contains(&w.window))
            .map(|(_, w)| w.id().get())
            .collect();
        let deadline = self
            .niri
            .layout
            .windows()
            .filter(|(_, w)| actual.contains(&w.window))
            .filter_map(|(_, w)| w.normal_restore_deadline())
            .min();
        let record = self
            .niri
            .shrink_expand
            .submit_immediate(workspace, targets, kind, deadline);
        self.ipc_refresh_shrink_expand();
        if let Some(window) = focus {
            self.cancel_expand_focus_lease();
            self.niri.layout.activate_window(&window);
            self.niri.shrink_expand_focus = Some(ExpandFocusLease {
                window,
                workspace,
                output: self.niri.layout.active_output().cloned(),
            });
        }
        self.refresh_shrink_expand_operations();
        self.niri.queue_redraw_all();
        Ok(Some(record))
    }

    fn cancel_expand_focus_lease(&mut self) {
        if let Some(lease) = self.niri.shrink_expand_focus.take() {
            if let Some(tile) = self
                .niri
                .layout
                .workspaces_mut()
                .flat_map(|ws| ws.tiles_mut())
                .find(|tile| tile.window().window == lease.window)
            {
                tile.cancel_normal_restore_focus();
            }
        }
    }

    fn refresh_expand_focus_lease(&mut self) {
        let Some(lease) = self.niri.shrink_expand_focus.as_ref() else {
            return;
        };
        let valid = self.niri.config.borrow().shrink_expand.enabled
            && self.can_expand_spines()
            && self.niri.layout.active_output() == lease.output.as_ref()
            && self.niri.layout.active_workspace().is_some_and(|ws| {
                ws.id().get() == lease.workspace
                    && ws
                        .selected_window()
                        .is_some_and(|w| w.window == lease.window)
                    && !self.niri.layout.is_spine(&lease.window)
            });
        if !valid {
            self.cancel_expand_focus_lease();
        } else if self
            .niri
            .layout
            .focus()
            .is_some_and(|w| w.window == lease.window)
        {
            self.niri.shrink_expand_focus = None;
            self.maybe_warp_cursor_to_focus();
        }
    }

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
        self.niri.layout.set_shrink_preview(
            &window,
            chosen.geometry.outer_size(),
            behavior.animate,
        );
        self.arm_shrink_preview_timer();
        self.niri.queue_redraw_all();
        self.ipc_refresh_shrink_expand();
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
        self.niri.layout.clear_shrink_previews();
        let record = self.niri.shrink_expand.cancel();
        if record.is_some() {
            self.niri.queue_redraw_all();
        }
        self.ipc_refresh_shrink_expand();
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
        self.ipc_refresh_shrink_expand();
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
        self.refresh_expand_focus_lease();
        let layout = &self.niri.layout;
        self.niri.shrink_expand.reconcile(Instant::now(), |id| {
            let Some((_, window)) = layout.windows().find(|(_, window)| window.id().get() == id)
            else {
                return TargetState::Removed;
            };
            if window.normal_restore_timed_out() {
                TargetState::Deadline
            } else if !window.normal_restore_ready() {
                TargetState::Waiting
            } else {
                TargetState::Ready
            }
        });
        self.ipc_refresh_shrink_expand();
    }
}
