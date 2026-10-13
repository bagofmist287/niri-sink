use std::time::{Duration, Instant};

use calloop::timer::{TimeoutAction, Timer};
use smithay::desktop::Window;
use smithay::output::Output;
use smithay::utils::{IsAlive, Logical, Point};
use smithay::wayland::input_method::InputMethodKeyboardGrab;

use super::{Kind, Record, TargetState, Ticket};
use crate::layout::shrink_expand::ShrinkAxis;
use crate::layout::spine_search::SpineSearchMode;
use crate::layout::LayoutElement as _;
use crate::niri::{KeyboardFocus, State};

#[derive(Debug, Clone)]
pub(crate) struct PreviewLease {
    output: Output,
    thickness: f64,
    border: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpatialInput {
    Pointer,
    Touch,
    Tablet,
}

#[derive(Debug)]
pub(crate) struct SpatialSession {
    window: Window,
    output: Output,
    workspace: u64,
    input: SpatialInput,
    delta: Point<f64, Logical>,
    zone: f64,
    selected: Option<ShrinkAxis>,
    inhibited: bool,
}

/// Identity and scene scope of one spine-body press; no layout or size history.
#[derive(Debug, Clone)]
pub(crate) struct SpineClick {
    pub window: Window,
    pub workspace: u64,
    pub output: Output,
    pub overview_open: bool,
    pub overview_generation: u64,
    pub cancel_only: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ExpandFocusLease {
    window: Window,
    workspace: u64,
    output: Option<Output>,
    resize_input: Option<SpatialInput>,
}

#[derive(Debug, Clone)]
pub(crate) struct ExpandSelectionLease {
    workspace: u64,
    output: Output,
    origin: Option<Window>,
    mode: SpineSearchMode,
    selected: Window,
    candidates: Vec<(Window, u64)>,
    overview_open: bool,
    overview_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    Disabled,
    Blocked,
    NoTarget,
    NoLegalAxis,
}

impl State {
    fn can_shrink_spatial(&self, input: SpatialInput) -> bool {
        !self.niri.is_locked()
            && self.niri.popup_grab.is_none()
            && !self.niri.window_mru_ui.is_open()
            && !self.niri.screenshot_ui.is_open()
            && !self.niri.exit_confirm_dialog.is_open()
            && matches!(
                self.niri.keyboard_focus,
                KeyboardFocus::Layout { .. } | KeyboardFocus::Overview
            )
            && self.niri.layout.can_transfer_sink()
            && !self.niri.seat.get_keyboard().is_some_and(|keyboard| {
                keyboard.is_grabbed()
                    && !keyboard
                        .with_grab(|_, grab| grab.is::<InputMethodKeyboardGrab>())
                        .unwrap_or(false)
            })
            && (input == SpatialInput::Pointer
                || !self
                    .niri
                    .seat
                    .get_pointer()
                    .is_some_and(|pointer| pointer.is_grabbed()))
            && (input == SpatialInput::Touch
                || !self
                    .niri
                    .seat
                    .get_touch()
                    .is_some_and(|touch| touch.is_grabbed()))
    }

    /// Called only by the compositor's existing ResizeGrab, while its own grab may be borrowed.
    pub(crate) fn spatial_resize_frame(
        &mut self,
        window: &Window,
        delta: Point<f64, Logical>,
        input: SpatialInput,
    ) -> bool {
        let behavior = self.niri.config.borrow().shrink_expand;
        if !behavior.enabled {
            if self
                .niri
                .shrink_spatial_session
                .as_ref()
                .is_some_and(|session| &session.window == window)
            {
                self.cancel_shrink_expand_preview();
            }
            return self.niri.layout.interactive_resize_update(window, delta);
        }
        let raw = self.niri.layout.spatial_resize_intent(window, delta);
        if !self.can_shrink_spatial(input) || raw.is_none() {
            if self
                .niri
                .shrink_spatial_session
                .as_ref()
                .is_some_and(|session| &session.window == window)
            {
                self.cancel_shrink_expand_preview();
            }
            return self.niri.layout.interactive_resize_update(window, delta);
        }
        let raw = raw.unwrap();
        let Some(output) = self.niri.layout.active_output().cloned() else {
            return self.niri.layout.interactive_resize_update(window, delta);
        };
        let Some(workspace) = self.niri.layout.active_workspace() else {
            return self.niri.layout.interactive_resize_update(window, delta);
        };
        if !workspace.has_window(window) {
            return self.niri.layout.interactive_resize_update(window, delta);
        }
        let workspace_id = workspace.id().get();
        let id = workspace
            .tiles()
            .find(|tile| &tile.window().window == window)
            .unwrap()
            .window()
            .id()
            .get();
        let border = workspace
            .tiles()
            .find(|tile| &tile.window().window == window)
            .unwrap()
            .effective_border_width();
        if self.niri.shrink_spatial_session.is_none() {
            self.cancel_shrink_expand_preview();
            self.niri.shrink_spatial_session = Some(SpatialSession {
                window: window.clone(),
                output,
                workspace: workspace_id,
                input,
                delta,
                zone: behavior.mouse_zone_px,
                selected: None,
                inhibited: false,
            });
        }
        let session = self.niri.shrink_spatial_session.as_mut().unwrap();
        if &session.window != window {
            return false;
        }
        session.delta = delta;
        if let Some(axis) = self.niri.layout.spine_axis(window) {
            if raw.grows_locked_axis(axis) {
                return self
                    .submit_spine_expansion_with_resize(
                        workspace_id,
                        vec![window.clone()],
                        Kind::ExpandDirect,
                        Some(window.clone()),
                        Some((delta, input)),
                    )
                    .is_ok();
            }
            return self.niri.layout.interactive_resize_update(window, delta);
        }
        let plans = self.niri.layout.shrink_plans(window, behavior.thickness);
        let legal = plans.map_or([false; 2], |plans| {
            [
                plans.get(ShrinkAxis::Horizontal).fits,
                plans.get(ShrinkAxis::Vertical).fits,
            ]
        });
        let chosen = raw.choose_axis(behavior.mouse_zone_px, legal, session.selected);
        if chosen.is_none() {
            session.selected = None;
            if self.niri.shrink_expand.preview().is_some() {
                self.cancel_shrink_expand_preview();
            }
            let session = self.niri.shrink_spatial_session.as_mut().unwrap();
            session.inhibited = false;
            session.zone = behavior.mouse_zone_px;
            return self.niri.layout.interactive_resize_update(window, delta);
        }
        if session.inhibited {
            return self.niri.layout.interactive_resize_update(window, delta);
        }
        let axis = chosen.unwrap();
        session.selected = Some(axis);
        if !self
            .niri
            .layout
            .interactive_resize_update(window, raw.held_delta(delta, axis, behavior.mouse_zone_px))
        {
            return false;
        }
        let kind = Kind::ShrinkSpatial(axis);
        if self.niri.shrink_expand.preview().is_none() {
            self.niri.shrink_expand.start(
                workspace_id,
                vec![id],
                kind,
                Instant::now(),
                Duration::ZERO,
            );
        } else if self.niri.shrink_expand.preview().unwrap().record.kind != kind {
            self.niri
                .shrink_expand
                .revise(kind, vec![id], Instant::now(), Duration::ZERO);
        }
        self.niri.shrink_preview_lease = Some(PreviewLease {
            output: self.niri.layout.active_output().unwrap().clone(),
            thickness: behavior.thickness,
            border,
        });
        let geometry = self
            .niri
            .layout
            .shrink_plans(window, behavior.thickness)
            .unwrap()
            .get(axis)
            .geometry;
        let mut pose = geometry.outer_size();
        match axis {
            ShrinkAxis::Horizontal => {
                pose.w = raw
                    .requested
                    .w
                    .clamp(pose.w, (raw.minimum.w + behavior.mouse_zone_px).max(pose.w))
            }
            ShrinkAxis::Vertical => {
                pose.h = raw
                    .requested
                    .h
                    .clamp(pose.h, (raw.minimum.h + behavior.mouse_zone_px).max(pose.h))
            }
        }
        if self.niri.layout.set_shrink_preview(window, pose, false) {
            self.niri.queue_redraw_all();
        }
        self.niri.layout.set_shrink_preview_anchor(
            window,
            Point::from((
                if raw.gain.x.abs() == 2. {
                    0.5
                } else if raw.gain.x < 0. {
                    1.
                } else {
                    0.
                },
                if raw.gain.y < 0. { 1. } else { 0. },
            )),
        );
        self.ipc_refresh_shrink_expand();
        true
    }

    pub(crate) fn spatial_release_needs_update(
        &self,
        window: &Window,
        delta: Point<f64, Logical>,
    ) -> bool {
        let behavior = self.niri.config.borrow().shrink_expand;
        if !behavior.enabled {
            return false;
        }
        if self
            .niri
            .shrink_expand
            .preview()
            .is_some_and(|preview| matches!(preview.record.kind, Kind::ShrinkSpatial(_)))
        {
            return true;
        }
        if self
            .niri
            .shrink_spatial_session
            .as_ref()
            .is_some_and(|session| &session.window == window && session.inhibited)
        {
            return false;
        }
        let Some(raw) = self.niri.layout.spatial_resize_intent(window, delta) else {
            return false;
        };
        if let Some(axis) = self.niri.layout.spine_axis(window) {
            return raw.grows_locked_axis(axis);
        }
        let Some(plans) = self.niri.layout.shrink_plans(window, behavior.thickness) else {
            return false;
        };
        raw.choose_axis(
            behavior.mouse_zone_px,
            [
                plans.get(ShrinkAxis::Horizontal).fits,
                plans.get(ShrinkAxis::Vertical).fits,
            ],
            None,
        )
        .is_some()
    }

    pub(crate) fn finish_spatial_resize(&mut self, window: &Window, released: bool) {
        let owned = self
            .niri
            .shrink_spatial_session
            .as_ref()
            .is_some_and(|session| &session.window == window);
        let submit = released
            && owned
            && self.shrink_preview_valid()
            && self
                .niri
                .shrink_expand
                .preview()
                .is_some_and(|preview| matches!(preview.record.kind, Kind::ShrinkSpatial(_)));
        let record = submit.then(|| self.niri.shrink_expand.preview().unwrap().record.clone());
        let thickness = self.niri.config.borrow().shrink_expand.thickness;
        self.niri.layout.interactive_resize_end(window);
        self.niri.shrink_spatial_session = None;
        if let Some(record) = record {
            let Kind::ShrinkSpatial(axis) = record.kind else {
                unreachable!()
            };
            if self.niri.layout.commit_shrink(window, axis, thickness) {
                self.niri.shrink_preview_lease = None;
                self.niri
                    .shrink_expand
                    .submit(record.ticket(), record.targets, None)
                    .unwrap();
                self.ipc_refresh_shrink_expand();
                self.refresh_shrink_expand_operations();
                self.niri.queue_redraw_all();
                return;
            }
        }
        if owned {
            self.cancel_shrink_expand_preview();
        }
    }

    /// Intercepts only eligible boundary/repeated Shrink intent before common grab side effects.
    pub(crate) fn handle_keyboard_shrink_boundary(&mut self, action: &niri_config::Action) -> bool {
        use niri_config::Action;
        use std::cmp::Ordering;
        if !self.niri.config.borrow().shrink_expand.enabled || self.niri.screenshot_ui.is_open() {
            return false;
        }
        let (axis, change, explicit_id) = match action {
            Action::SetColumnWidth(change) | Action::SetWindowWidth(change) => {
                (ShrinkAxis::Horizontal, *change, None)
            }
            Action::SetWindowHeight(change) => (ShrinkAxis::Vertical, *change, None),
            Action::SetWindowWidthById { id, change } => {
                (ShrinkAxis::Horizontal, *change, Some(*id))
            }
            Action::SetWindowHeightById { id, change } => {
                (ShrinkAxis::Vertical, *change, Some(*id))
            }
            _ => return false,
        };
        if self.niri.shrink_expand.preview().is_some() && !self.shrink_preview_valid() {
            self.cancel_shrink_expand_preview();
        }
        let Some(target) = self.niri.layout.focus() else {
            return false;
        };
        if explicit_id.is_some_and(|id| id != target.id().get()) {
            return false;
        }
        let window = target.window.clone();
        let Some(intent) = self
            .niri
            .layout
            .keyboard_resize_intent(&window, axis, change)
        else {
            return false;
        };
        let selected_axis = self.niri.shrink_expand.preview().and_then(|preview| {
            if let Kind::ShrinkTimed(axis) = preview.record.kind {
                Some(axis)
            } else {
                None
            }
        });
        if selected_axis == Some(axis) && intent.direction == Ordering::Greater {
            self.cancel_shrink_expand_preview();
            return false;
        }
        if intent.direction != Ordering::Less || (!intent.at_minimum && selected_axis != Some(axis))
        {
            return false;
        }
        match self.begin_shrink_preview(Some(intent.axis)) {
            Ok(_) | Err(Rejected::Blocked) => true,
            Err(_) => false,
        }
    }

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
                | Action::ExpandNearbyWindow
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
            Action::ExpandNearbyWindow => self.begin_expand_selection().map(Some),
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

    pub(crate) fn spine_interaction_hit(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(Window, crate::utils::ResizeEdge)> {
        if !self.niri.config.borrow().shrink_expand.enabled || !self.can_expand_spines() {
            return None;
        }
        let (window, crate::layout::HitType::Spine { edges }) =
            self.niri.contents_under(pos).window?
        else {
            return None;
        };
        self.niri
            .layout
            .is_spine(&window)
            .then_some((window, edges))
    }

    pub(crate) fn spine_click_context(
        &self,
        window: &Window,
        pos: Point<f64, Logical>,
    ) -> Option<SpineClick> {
        if !self.niri.layout.is_spine(window) {
            return None;
        }
        let (hit, edges) = self.spine_interaction_hit(pos)?;
        if &hit != window || !edges.is_empty() {
            return None;
        }
        let (mon, _, ws) = self
            .niri
            .layout
            .workspaces()
            .find(|(_, _, ws)| ws.has_window(window))?;
        Some(SpineClick {
            window: window.clone(),
            workspace: ws.id().get(),
            output: mon?.output().clone(),
            overview_open: self.niri.layout.is_overview_open(),
            overview_generation: self.niri.layout.overview_generation(),
            cancel_only: self.niri.shrink_expand.preview().is_some(),
        })
    }

    pub(crate) fn spine_press_scope_valid(&self, click: &SpineClick) -> bool {
        self.niri.config.borrow().shrink_expand.enabled
            && click.window.alive()
            && self.niri.layout.is_spine(&click.window)
            && self.niri.layout.is_overview_open() == click.overview_open
            && self.niri.layout.overview_generation() == click.overview_generation
            && self.niri.layout.workspaces().any(|(mon, _, ws)| {
                ws.id().get() == click.workspace
                    && ws.has_window(&click.window)
                    && mon.is_some_and(|mon| mon.output() == &click.output)
            })
    }

    pub(crate) fn queue_spine_click(&mut self, click: SpineClick, position: Point<f64, Logical>) {
        self.niri.event_loop.insert_idle(move |state| {
            if click.cancel_only
                || !click.window.alive()
                || !state.can_expand_spines()
                || !state.niri.config.borrow().shrink_expand.enabled
                || state.niri.layout.is_overview_open() != click.overview_open
                || state.niri.layout.overview_generation() != click.overview_generation
                || state
                    .niri
                    .contents_under(position)
                    .window
                    .is_none_or(|(window, hit)| {
                        window != click.window
                            || !matches!(hit, crate::layout::HitType::Spine { .. })
                    })
            {
                return;
            }
            let target = state.niri.layout.workspaces().find_map(|(mon, idx, ws)| {
                (ws.id().get() == click.workspace
                    && ws.has_window(&click.window)
                    && mon.is_some_and(|mon| mon.output() == &click.output))
                .then(|| {
                    (
                        idx,
                        ws.windows()
                            .find(|w| w.window == click.window)
                            .unwrap()
                            .id()
                            .get(),
                    )
                })
            });
            let Some((idx, id)) = target else {
                return;
            };
            if click.overview_open {
                state.niri.layout.focus_output(&click.output);
                state.niri.layout.toggle_overview_to_workspace(idx);
            }
            let _ = state.expand_spine_window(id);
        });
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
        self.submit_spine_expansion_with_resize(workspace, windows, kind, focus, None)
    }

    fn submit_spine_expansion_with_resize(
        &mut self,
        workspace: u64,
        windows: Vec<Window>,
        kind: Kind,
        focus: Option<Window>,
        resize: Option<(Point<f64, Logical>, SpatialInput)>,
    ) -> Result<Option<Record>, Rejected> {
        let ws = self
            .niri
            .layout
            .workspaces_mut()
            .find(|ws| ws.id().get() == workspace)
            .ok_or(Rejected::NoTarget)?;
        let actual = if let Some((delta, _)) = resize {
            if ws.restore_spine_for_resize(&windows[0], delta) {
                windows.clone()
            } else {
                Vec::new()
            }
        } else {
            ws.restore_normal_spines(&windows)
        };
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
        let record = if kind == Kind::ExpandSelection {
            let ticket = self.niri.shrink_expand.preview().unwrap().record.ticket();
            self.niri
                .shrink_expand
                .submit(ticket, targets, deadline)
                .unwrap()
        } else {
            self.niri
                .shrink_expand
                .submit_immediate(workspace, targets, kind, deadline)
        };
        self.ipc_refresh_shrink_expand();
        if let Some(window) = focus {
            self.cancel_expand_focus_lease();
            if resize.is_none() {
                self.niri.layout.activate_window(&window);
            }
            self.niri.shrink_expand_focus = Some(ExpandFocusLease {
                window,
                workspace,
                output: self.niri.layout.active_output().cloned(),
                resize_input: resize.map(|(_, input)| input),
            });
        }
        self.refresh_shrink_expand_operations();
        self.niri.queue_redraw_all();
        Ok(Some(record))
    }

    /// Lifecycle invalidation is allowed under lock/grabs and never activates a restored book.
    pub(crate) fn restore_spines_on_disable(&mut self) {
        let focus = self.niri.layout.focus().map(|window| window.window.clone());
        self.cancel_shrink_expand_preview();
        self.cancel_expand_focus_lease();
        let last_motion = self
            .niri
            .shrink_spatial_session
            .as_ref()
            .map(|session| (session.window.clone(), session.delta));
        let batches = self
            .niri
            .layout
            .restore_all_spines_on_disable(last_motion.as_ref().map(|(id, delta)| (id, *delta)));
        self.niri.shrink_spatial_session = None;
        for (workspace, ids) in batches {
            let windows = self
                .niri
                .layout
                .windows()
                .filter(|(_, win)| ids.contains(&win.window))
                .map(|(_, win)| win)
                .collect::<Vec<_>>();
            let targets = windows.iter().map(|win| win.id().get()).collect();
            let deadline = windows
                .iter()
                .filter_map(|win| win.normal_restore_deadline())
                .min();
            self.niri
                .shrink_expand
                .submit_immediate(workspace, targets, Kind::ExpandAll, deadline);
        }
        if let Some(focus) = focus {
            self.niri.layout.activate_window(&focus);
        }
        self.ipc_refresh_shrink_expand();
        self.refresh_shrink_expand_operations();
        self.niri.queue_redraw_all();
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
        // This may run inside ResizeGrab::frame while the originating device is borrowed.
        // Never ask the generic seat guard to borrow that device again.
        let resizing = lease.resize_input.filter(|input| {
            self.niri
                .shrink_spatial_session
                .as_ref()
                .is_some_and(|session| session.window == lease.window && session.input == *input)
        });
        let seat_allowed = if let Some(input) = resizing {
            self.can_shrink_spatial(input)
        } else {
            self.can_expand_spines()
        };
        let valid = self.niri.config.borrow().shrink_expand.enabled
            && seat_allowed
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
            if resizing.is_none() {
                self.maybe_warp_cursor_to_focus();
            }
        }
    }

    fn can_start_shrink_preview(&self) -> bool {
        self.can_recall_sink(false)
            && matches!(self.niri.keyboard_focus, KeyboardFocus::Layout { .. })
            && !self.niri.screenshot_ui.is_open()
            && !self.niri.exit_confirm_dialog.is_open()
    }

    fn expand_selection_candidates(
        &self,
        lease: &ExpandSelectionLease,
    ) -> Option<Vec<(Window, u64)>> {
        if !self.niri.config.borrow().shrink_expand.enabled
            || !self.can_expand_spines()
            || self.niri.layout.active_output() != Some(&lease.output)
            || self.niri.layout.is_overview_open() != lease.overview_open
            || self.niri.layout.overview_generation() != lease.overview_generation
            || self.niri.layout.focus().map(|win| &win.window) != lease.origin.as_ref()
        {
            return None;
        }
        let ws = self.niri.layout.active_workspace()?;
        if ws.id().get() != lease.workspace {
            return None;
        }
        let search = ws.spine_candidates(lease.origin.as_ref(), |a, b| {
            a.id().get().cmp(&b.id().get())
        })?;
        if search.mode != lease.mode || search.candidates.is_empty() {
            return None;
        }
        let candidates = search
            .candidates
            .into_iter()
            .map(|candidate| {
                let tile = ws
                    .tiles()
                    .find(|tile| tile.window().window == candidate.id)
                    .unwrap();
                (candidate.id, tile.window().id().get())
            })
            .collect::<Vec<_>>();
        candidates
            .iter()
            .any(|(window, _)| window == &lease.selected)
            .then_some(candidates)
    }

    /// Shared nearby command entry: selection preserves the original ordinary focus.
    pub fn begin_expand_selection(&mut self) -> Result<Record, Rejected> {
        if !self.niri.config.borrow().shrink_expand.enabled {
            return Err(Rejected::Disabled);
        }
        if let Some(preview) = self.niri.shrink_expand.preview() {
            if preview.record.kind != Kind::ExpandSelection || !self.shrink_preview_valid() {
                return Ok(self.cancel_shrink_expand_preview().unwrap());
            }
        }
        if !self.can_expand_spines() {
            return Err(Rejected::Blocked);
        }
        let lease = if let Some(lease) = self.niri.expand_selection_lease.as_ref() {
            let mut lease = lease.clone();
            let candidates = self.expand_selection_candidates(&lease).unwrap();
            let index = candidates
                .iter()
                .position(|(window, _)| window == &lease.selected)
                .unwrap();
            lease.selected = candidates[(index + 1) % candidates.len()].0.clone();
            lease.candidates = candidates;
            lease
        } else {
            let ws = self
                .niri
                .layout
                .active_workspace()
                .ok_or(Rejected::NoTarget)?;
            let origin = self.niri.layout.focus().map(|win| win.window.clone());
            let search = ws
                .spine_candidates(origin.as_ref(), |a, b| a.id().get().cmp(&b.id().get()))
                .ok_or(Rejected::NoTarget)?;
            let candidates = search
                .candidates
                .into_iter()
                .map(|candidate| {
                    let tile = ws
                        .tiles()
                        .find(|tile| tile.window().window == candidate.id)
                        .unwrap();
                    (candidate.id, tile.window().id().get())
                })
                .collect::<Vec<_>>();
            let selected = candidates.first().ok_or(Rejected::NoTarget)?.0.clone();
            ExpandSelectionLease {
                workspace: ws.id().get(),
                output: self
                    .niri
                    .layout
                    .active_output()
                    .ok_or(Rejected::NoTarget)?
                    .clone(),
                origin,
                mode: search.mode,
                selected,
                candidates,
                overview_open: self.niri.layout.is_overview_open(),
                overview_generation: self.niri.layout.overview_generation(),
            }
        };
        let selected_id = lease
            .candidates
            .iter()
            .find(|(window, _)| window == &lease.selected)
            .unwrap()
            .1;
        let delay = Duration::from_millis(u64::from(
            self.niri.config.borrow().shrink_expand.expand_buffer_ms,
        ));
        let now = Instant::now();
        let record = if self.niri.shrink_expand.preview().is_some() {
            self.niri
                .shrink_expand
                .revise(Kind::ExpandSelection, vec![selected_id], now, delay)
                .unwrap()
        } else {
            self.niri.shrink_expand.start(
                lease.workspace,
                vec![selected_id],
                Kind::ExpandSelection,
                now,
                delay,
            )
        };
        // Store soft identities only; no layout/window ownership is transferred by selection.
        let behavior = self.niri.config.borrow().shrink_expand;
        self.niri.layout.set_expand_preview(
            &lease.selected,
            behavior.preview_scale,
            behavior.animate,
        );
        self.niri.expand_selection_lease = Some(lease);
        self.arm_shrink_preview_timer();
        self.ipc_refresh_shrink_expand();
        self.niri.queue_redraw_all();
        Ok(record)
    }

    fn confirm_expand_selection(&mut self, ticket: Ticket) -> Option<Record> {
        if !self.shrink_preview_valid() {
            return self.cancel_shrink_expand_preview();
        }
        let lease = self.niri.expand_selection_lease.take().unwrap();
        self.niri.layout.commit_expand_preview(&lease.selected);
        if let Some((token, _)) = self.niri.shrink_preview_timer.take() {
            self.niri.event_loop.remove(token);
        }
        // The validated preview record is submitted by the same native restoration path.
        debug_assert!(self.niri.shrink_expand.matches(ticket));
        self.submit_spine_expansion(
            lease.workspace,
            vec![lease.selected.clone()],
            Kind::ExpandSelection,
            Some(lease.selected),
        )
        .ok()
        .flatten()
        .or_else(|| self.cancel_shrink_expand_preview())
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
        if preview.record.kind == Kind::ExpandSelection {
            return self
                .niri
                .expand_selection_lease
                .as_ref()
                .is_some_and(|lease| {
                    self.expand_selection_candidates(lease).is_some()
                        && lease.workspace == preview.record.workspace
                        && lease
                            .candidates
                            .iter()
                            .find(|(window, _)| window == &lease.selected)
                            .is_some_and(|(_, id)| preview.record.targets.as_slice() == [*id])
                });
        }
        let Some(lease) = &self.niri.shrink_preview_lease else {
            return false;
        };
        let (axis, permitted) = match preview.record.kind {
            Kind::ShrinkTimed(axis) => (axis, self.can_start_shrink_preview()),
            Kind::ShrinkSpatial(axis) => {
                let Some(session) = &self.niri.shrink_spatial_session else {
                    return false;
                };
                let valid = !session.inhibited
                    && self.can_shrink_spatial(session.input)
                    && self.niri.layout.active_output() == Some(&session.output)
                    && session.workspace == preview.record.workspace
                    && session.zone == self.niri.config.borrow().shrink_expand.mouse_zone_px
                    && self
                        .niri
                        .layout
                        .spatial_resize_intent(&session.window, session.delta)
                        .is_some();
                (axis, valid)
            }
            _ => return false,
        };
        if !self.niri.config.borrow().shrink_expand.enabled
            || !permitted
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
        if self
            .niri
            .shrink_expand
            .preview()
            .is_some_and(|preview| matches!(preview.record.kind, Kind::ShrinkSpatial(_)))
        {
            if let Some(session) = &mut self.niri.shrink_spatial_session {
                session.inhibited = true;
            }
        }
        if let Some((token, _)) = self.niri.shrink_preview_timer.take() {
            self.niri.event_loop.remove(token);
        }
        self.niri.shrink_preview_lease = None;
        if let Some(lease) = self.niri.expand_selection_lease.take() {
            if !self.niri.is_locked()
                && self.niri.layout.can_transfer_sink()
                && self.niri.layout.active_output() == Some(&lease.output)
                && self
                    .niri
                    .layout
                    .active_workspace()
                    .is_some_and(|ws| ws.id().get() == lease.workspace)
                && self.niri.layout.focus().map(|win| &win.window) == lease.origin.as_ref()
                && self.niri.layout.is_overview_open() == lease.overview_open
                && self.niri.layout.overview_generation() == lease.overview_generation
            {
                if let Some(origin) = lease.origin {
                    self.niri.layout.reveal_expand_origin(&origin);
                }
            }
        }
        self.niri.layout.clear_shrink_previews();
        self.niri.layout.clear_expand_previews();
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
        if self.niri.shrink_expand.preview().unwrap().record.kind == Kind::ExpandSelection {
            return self.confirm_expand_selection(ticket);
        }
        if self
            .niri
            .shrink_expand
            .preview()
            .is_some_and(|preview| matches!(preview.record.kind, Kind::ShrinkSpatial(_)))
        {
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
        if self.niri.layout.finish_ready_normal_restores() {
            self.niri.queue_redraw_all();
        }
        if self.niri.shrink_expand.preview().is_some() && !self.shrink_preview_valid() {
            self.cancel_shrink_expand_preview();
        }
        if let Some(lease) = self.niri.expand_selection_lease.as_ref() {
            if let Some(candidates) = self.expand_selection_candidates(lease) {
                self.niri
                    .expand_selection_lease
                    .as_mut()
                    .unwrap()
                    .candidates = candidates;
            }
        }
        if let Some(lease) = &self.niri.expand_selection_lease {
            let behavior = self.niri.config.borrow().shrink_expand;
            if self.niri.layout.set_expand_preview(
                &lease.selected,
                behavior.preview_scale,
                behavior.animate,
            ) {
                self.niri.queue_redraw_all();
            }
        }
        if let Some(preview) = self.niri.shrink_expand.preview() {
            if let Kind::ShrinkTimed(axis) = preview.record.kind {
                let window = self.niri.layout.focus().unwrap().window.clone();
                let behavior = self.niri.config.borrow().shrink_expand;
                let plan = self
                    .niri
                    .layout
                    .shrink_plans(&window, behavior.thickness)
                    .unwrap()
                    .get(axis);
                if self.niri.layout.set_shrink_preview(
                    &window,
                    plan.geometry.outer_size(),
                    behavior.animate,
                ) {
                    self.niri.queue_redraw_all();
                }
            }
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
