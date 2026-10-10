use super::*;
use crate::layout::sink::{RestorePlacement, SinkEntry, SinkSpace};
use crate::layout::workspace::Workspace;
use approx::assert_abs_diff_eq;

fn entry(id: usize) -> SinkEntry<TestWindow> {
    let workspace = Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::default());
    let tile = workspace.make_tile(TestWindow::new(TestWindowParams::new(id)));
    SinkEntry::new(
        tile,
        RestorePlacement::Scrolling {
            width: ColumnWidth::Fixed(400.),
            is_full_width: false,
            height: super::super::scrolling::WindowHeight::Fixed(180.),
        },
        (20., 40.).into(),
    )
}

#[test]
fn lifo_and_depths_follow_ownership() {
    let mut sink = SinkSpace::default();
    for id in 1..=3 {
        sink.push(entry(id)).unwrap();
    }
    assert_eq!(sink.len(), 3);
    assert_eq!(sink.depth(&3), Some(0));
    assert_eq!(sink.depth(&2), Some(1));
    assert_eq!(sink.depth(&1), Some(2));
    assert_eq!(
        sink.entries()
            .map(|(depth, entry)| (depth, *entry.tile().window().id()))
            .collect::<Vec<_>>(),
        [(0, 3), (1, 2), (2, 1)]
    );
    for id in (1..=3).rev() {
        let entry = sink.pop().unwrap();
        assert_eq!(entry.tile().window().id(), &id);
        assert_eq!(sink.depth(&id), None);
        assert_eq!(sink.len(), id - 1);
    }
    assert!(sink.is_empty());
    assert!(sink.pop().is_none());
}

#[test]
fn removing_any_depth_keeps_remaining_order() {
    for removed_id in 1..=3 {
        let mut sink = SinkSpace::default();
        for id in 1..=3 {
            sink.push(entry(id)).unwrap();
        }
        let removed = sink.remove(&removed_id).unwrap();
        assert_eq!(removed.tile().window().id(), &removed_id);
        assert!(sink.remove(&removed_id).is_none());
        assert!(sink.remove(&99).is_none());
        let expected = (1..=3)
            .rev()
            .filter(|id| *id != removed_id)
            .collect::<Vec<_>>();
        assert_eq!(
            sink.entries()
                .map(|(_, entry)| *entry.tile().window().id())
                .collect::<Vec<_>>(),
            expected
        );
        for (depth, id) in expected.iter().enumerate() {
            assert_eq!(sink.depth(id), Some(depth));
        }
    }
}

#[test]
fn duplicate_rejection_returns_incoming_owner() {
    let mut sink = SinkSpace::default();
    let original = entry(1);
    let original_window = original.tile().window().clone();
    sink.push(original).unwrap();
    let incoming = entry(1);
    let incoming_window = incoming.tile().window().clone();
    let rejected = sink.push(incoming).unwrap_err();
    assert_eq!(sink.len(), 1);
    assert!(Rc::ptr_eq(
        &sink.get(&1).unwrap().tile().window().0,
        &original_window.0
    ));
    assert!(Rc::ptr_eq(&rejected.tile().window().0, &incoming_window.0));
}

#[test]
fn source_geometry_and_restore_metadata_survive_storage() {
    let mut sink = SinkSpace::default();
    let mut item = entry(1);
    item.tile_mut().restore_to_floating = true;
    let source = item.source;
    let restore = item.restore;
    let window = item.tile().window().clone();
    sink.push(item).unwrap();
    assert_eq!(sink.get(&1).unwrap().source, source);
    assert_eq!(sink.get(&1).unwrap().source.tile.loc, (20., 40.).into());
    let item = sink.pop().unwrap();
    let (tile, restored, geometry) = item.into_parts();
    assert_eq!(restored, restore);
    assert_eq!(geometry, source);
    assert!(tile.restore_to_floating);
    assert!(Rc::ptr_eq(&tile.window().0, &window.0));
    assert_eq!(window.0.requested_size.get(), None);
    assert!(window.0.output_events.borrow().is_empty());
}

use crate::layout::scrolling::WindowHeight;
use crate::layout::workspace::WorkspaceAddWindowTarget;

fn add_window(workspace: &mut Workspace<TestWindow>, id: usize, floating: bool) -> TestWindow {
    let mut params = TestWindowParams::new(id);
    params.is_floating = floating;
    let window = TestWindow::new(params);
    workspace.add_tile(
        workspace.make_tile(window.clone()),
        WorkspaceAddWindowTarget::Auto,
        ActivateWindow::Yes,
        ColumnWidth::Fixed(400.),
        false,
        floating,
        None,
    );
    settle(workspace);
    window
}

fn settle(workspace: &mut Workspace<TestWindow>) {
    let changed = workspace
        .windows()
        .filter_map(|win| win.communicate().then_some(*win.id()))
        .collect::<Vec<_>>();
    for id in changed {
        workspace.update_window(&id, None);
    }
    workspace.refresh(true, true);
    workspace.verify_invariants(None);
}

#[test]
fn workspace_only_sink_keeps_inventory_and_output_but_has_no_normal_focus() {
    for floating in [false, true] {
        let mut layout = Layout::<TestWindow>::default();
        Op::AddOutput(1).apply(&mut layout);
        let output = layout.outputs().next().unwrap().clone();
        let mut workspace = Workspace::new(output, Clock::with_time(Duration::ZERO), Rc::default());
        let win = add_window(&mut workspace, 1, floating);
        win.0.output_events.borrow_mut().clear();
        let size = win.size();
        let requested = win.requested_size();
        assert!(workspace.sink_window(&1));
        settle(&mut workspace);
        assert!(workspace.has_windows_or_name());
        assert!(workspace.has_window(&1));
        assert_eq!(workspace.windows().count(), 1);
        assert_eq!(workspace.sink_count(), 1);
        assert_eq!(workspace.sink_depth(&1), Some(0));
        assert_eq!(workspace.is_floating(&1), floating);
        assert!(workspace.active_window().is_none());
        assert!(!win.0.pending_activated.get());
        assert_eq!(workspace.tiles_with_render_positions().count(), 0);
        assert!(workspace.window_under((100., 100.).into()).is_none());
        let (_, ipc) = workspace.tiles_with_ipc_layouts().next().unwrap();
        assert!(ipc.pos_in_scrolling_layout.is_none());
        assert_eq!(win.size(), size);
        assert_eq!(win.requested_size(), requested);
        assert!(win.0.output_events.borrow().is_empty());
        assert!(!workspace.activate_window(&1));
        assert!(!workspace.activate_window_without_raising(&1));
        assert_eq!(workspace.sink_depth(&1), Some(0));
        assert_eq!(workspace.pull_window(true), Some(1));
        settle(&mut workspace);
        assert_eq!(workspace.active_window().map(LayoutElement::id), Some(&1));
        assert_eq!(workspace.is_floating(&1), floating);
        assert!(win.0.output_events.borrow().is_empty());
        assert!(Rc::ptr_eq(&win.0, &workspace.windows().next().unwrap().0));
        assert_eq!(workspace.pull_window(true), None);
    }
}

#[test]
fn overview_projection_hits_follow_lifo_and_visible_pose_without_input() {
    for scale in [1., 1.25, 1.5] {
        let mut config = Config::default();
        config.sink.enabled = true;
        let mut layout = Layout::<TestWindow>::new(Clock::with_time(Duration::ZERO), &config);
        Op::AddOutput(1).apply(&mut layout);
        let output = layout.outputs().next().unwrap().clone();
        output.change_current_state(
            None,
            None,
            Some(smithay::output::Scale::Fractional(scale)),
            None,
        );
        layout.update_output_size(&output);
        let ws = layout.active_workspace_mut().unwrap();
        for id in 1..=3 {
            add_window(ws, id, true);
            assert!(ws.sink_window(&id));
        }
        layout.clock.set_complete_instantly(true);
        layout.advance_animations();
        layout.clock.set_complete_instantly(false);
        let ws = layout.active_workspace().unwrap();
        let top = ws.tiles().find(|tile| tile.window().id() == &3).unwrap();
        let pose = top.sink_presentation.as_ref().unwrap().pose();
        let center = pose.rectangle(top.animated_tile_size()).loc
            + pose
                .rectangle(top.animated_tile_size())
                .size
                .to_point()
                .downscale(2.);
        assert_eq!(
            ws.sink_window_under(center).map(|(w, h)| (*w.id(), h)),
            Some((
                3,
                HitType::Activate {
                    is_tab_indicator: false
                }
            ))
        );
        assert!(ws.window_under(center).is_none());
        assert!(layout.window_under(&output, center).is_none());
        layout.open_overview();
        Op::CompleteAnimations.apply(&mut layout);
        let mon = layout.monitor_for_output(&output).unwrap();
        let (ws, geo) = mon
            .workspaces_with_render_geo()
            .find(|(ws, _)| ws.sink_count() == 3)
            .unwrap();
        // Expanded Overview selects its exposed top entry, not the old desktop projection center.
        let point = geo.loc
            + Point::from((
                ws.view_size().w * mon.overview_zoom() / 2.,
                -config.overview.sink.entry_height / 2.,
            ));
        assert_eq!(
            layout
                .window_under(&output, point)
                .map(|(w, h)| (*w.id(), h)),
            Some((
                3,
                HitType::Activate {
                    is_tab_indicator: false
                }
            ))
        );
        assert_eq!(ws.sink_count(), 3);
        layout.close_overview();
        assert!(layout.window_under(&output, point).is_none());
    }
}

#[test]
fn overview_expansion_preserves_sources_aligns_foreground_and_limits_tail_hits() {
    for depth in 1..=4 {
        for scale in [1., 1.25, 1.5] {
            for foreground in 0..4 {
                let mut config = Config::default();
                config.sink.enabled = true;
                config.sink.visible_depth = depth;
                let mut layout =
                    Layout::<TestWindow>::new(Clock::with_time(Duration::ZERO), &config);
                Op::AddOutput(1).apply(&mut layout);
                let output = layout.outputs().next().unwrap().clone();
                output.change_current_state(
                    None,
                    None,
                    Some(smithay::output::Scale::Fractional(scale)),
                    None,
                );
                layout.update_output_size(&output);
                for (index, size) in [
                    (320, 200),
                    (700, 120),
                    (200, 650),
                    (900, 900),
                    (180, 400),
                    (600, 300),
                ]
                .into_iter()
                .enumerate()
                {
                    let id = index + 1;
                    let mut params = TestWindowParams::new(id);
                    params.is_floating = true;
                    params.bbox = Rectangle::from_size(size.into());
                    Op::AddWindow { params }.apply(&mut layout);
                    Op::SetForcedSize {
                        id,
                        size: Some(size.into()),
                    }
                    .apply(&mut layout);
                    settle_system_layout(&mut layout);
                    assert!(layout.sink_window());
                }
                if foreground != 0 {
                    let mut params = TestWindowParams::new(7);
                    params.is_floating = foreground == 2;
                    Op::AddWindow { params }.apply(&mut layout);
                    if foreground == 3 {
                        layout.set_fullscreen(&7, true);
                    }
                    settle_system_layout(&mut layout);
                }
                Op::CompleteAnimations.apply(&mut layout);
                let before: Vec<_> = layout
                    .windows()
                    .map(|(_, win)| (*win.id(), win.size(), win.requested_size()))
                    .collect();
                assert!(layout.open_overview());
                Op::CompleteAnimations.apply(&mut layout);
                let mon = layout.monitor_for_output(&output).unwrap();
                let (ws, geo) = mon
                    .workspaces_with_render_geo()
                    .find(|(ws, _)| ws.sink_count() == 6)
                    .unwrap();
                let zoom = mon.overview_zoom();
                let requested = depth as f64 * 24. + 12.;
                let fraction = (ws.view_size().h * zoom * 0.4 / requested).min(1.);
                let mut h = 24. * fraction;
                for id in (7 - depth..=6).rev() {
                    let tile = ws.tiles().find(|tile| tile.window().id() == &id).unwrap();
                    let source = tile.animated_tile_size();
                    let fit = (ws.view_size().w / source.w)
                        .min(ws.view_size().h / source.h)
                        .min(1.);
                    h = h.min(source.h * fit * zoom);
                }
                let extent =
                    depth as f64 * h + 12. * fraction * (1. - 0.5f64.powi((6 - depth) as i32));
                assert_abs_diff_eq!(
                    ws.sink_overview_foreground_offset() * zoom,
                    extent / 2.,
                    epsilon = 1e-8
                );
                for d in 0..6 {
                    let id = 6 - d;
                    let tile = ws.tiles().find(|tile| tile.window().id() == &id).unwrap();
                    let pose = ws.sink_render_pose(d, tile);
                    assert_eq!(pose.is_some(), d < depth || d == 5);
                    if let Some(pose) = pose {
                        assert_eq!(pose.alpha, 1.);
                        let rect = pose.rectangle(tile.animated_tile_size());
                        assert_abs_diff_eq!(
                            rect.loc.x + rect.size.w / 2.,
                            ws.view_size().w / 2.,
                            epsilon = 1e-8
                        );
                        if d < depth {
                            assert_abs_diff_eq!(
                                rect.loc.y * zoom,
                                -((d + 1) as f64) * h,
                                epsilon = 1e-8
                            );
                            let point = geo.loc
                                + Point::from((
                                    rect.loc.x + rect.size.w / 2.,
                                    rect.loc.y + h / zoom / 2.,
                                ))
                                .upscale(zoom);
                            assert_eq!(
                                layout
                                    .window_under(&output, point)
                                    .map(|(win, hit)| (*win.id(), hit)),
                                Some((
                                    id,
                                    HitType::Activate {
                                        is_tab_indicator: false
                                    }
                                ))
                            );
                            assert_eq!(
                                mon.workspace_under(point).map(|(ws, _)| ws.id()),
                                Some(ws.id())
                            );
                            assert_eq!(
                                mon.workspace_under_narrow(point).map(Workspace::id),
                                Some(ws.id())
                            );
                        } else {
                            // The aggregated tail is a visible hint, never an arbitrary-depth recall.
                            let point = geo.loc
                                + Point::from((
                                    rect.loc.x + rect.size.w / 2.,
                                    rect.loc.y + 0.1 / zoom,
                                ))
                                .upscale(zoom);
                            assert!(layout.window_under(&output, point).is_none());
                        }
                    }
                }
                let after: Vec<_> = layout
                    .windows()
                    .map(|(_, win)| (*win.id(), win.size(), win.requested_size()))
                    .collect();
                assert_eq!(before, after);
                assert!(layout.window_under(&output, (-1., 20.).into()).is_none());
                layout.verify_invariants();
            }
        }
    }
}

#[test]
fn overview_capacity_covers_empty_workspaces_and_does_not_collapse_on_local_changes() {
    let mut options = Options::default();
    options.sink.enabled = true;
    let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    settle_system_layout(&mut layout);
    layout.open_overview();
    Op::CompleteAnimations.apply(&mut layout);
    let output = layout.outputs().next().unwrap().clone();
    let geos_before: Vec<_> = layout
        .monitor_for_output(&output)
        .unwrap()
        .workspaces_render_geo()
        .collect();
    assert!(layout.sink_window());
    let mon = layout.monitor_for_output(&output).unwrap();
    let geos_after: Vec<_> = mon.workspaces_render_geo().collect();
    assert_eq!(geos_before.len(), geos_after.len());
    // The first Sink changes only that card's foreground displacement. Other origins stay put.
    for (idx, ws) in mon.workspaces.iter().enumerate() {
        if ws.sink_count() == 0 {
            assert_eq!(geos_before[idx], geos_after[idx]);
        }
    }
    let (ws, geo) = mon
        .workspaces_with_render_geo()
        .find(|(ws, _)| ws.sink_count() != 0)
        .unwrap();
    let bounds = mon.workspace_bounds(ws, geo);
    let id = ws.id();
    assert!(layout.pull_window().is_some());
    assert!(layout.is_overview_open());
    let mon = layout.monitor_for_output(&output).unwrap();
    let (ws, geo) = mon
        .workspaces_with_render_geo()
        .find(|(ws, _)| ws.id() == id)
        .unwrap();
    let empty_bounds = mon.workspace_bounds(ws, geo);
    assert_abs_diff_eq!(empty_bounds.loc.y, bounds.loc.y, epsilon = 1e-8);
    assert_eq!(empty_bounds.size, bounds.size);
    layout.verify_invariants();
}

#[test]
fn overview_small_mixed_outputs_share_allocations_with_navigation_and_drop() {
    use crate::layout::monitor::InsertWorkspace;

    let mut options = Options::default();
    options.sink.enabled = true;
    options.sink.visible_depth = 4;
    options.overview.sink.entry_height = 256.;
    options.overview.sink.tail_height = 256.;
    let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
    for out in 1..=2 {
        Op::AddOutput(out).apply(&mut layout);
        let output = layout
            .outputs()
            .find(|o| o.name() == format!("output{out}"))
            .unwrap()
            .clone();
        output.change_current_state(
            Some(smithay::output::Mode {
                size: (320, 240).into(),
                refresh: 60000,
            }),
            None,
            Some(output::Scale::Fractional(if out == 1 { 1.25 } else { 1.5 })),
            None,
        );
        layout.update_output_size(&output);
        layout.focus_output(&output);
        for group in 0..3 {
            for n in 0..3 {
                let id = out * 100 + group * 10 + n;
                Op::AddWindow {
                    params: TestWindowParams::new(id),
                }
                .apply(&mut layout);
                settle_system_layout(&mut layout);
                if group % 2 == 0 {
                    assert!(layout.sink_window());
                }
            }
            layout.switch_workspace_down();
            Op::CompleteAnimations.apply(&mut layout);
        }
    }
    assert!(layout.open_overview());
    Op::CompleteAnimations.apply(&mut layout);
    let outputs: Vec<_> = layout.outputs().cloned().collect();
    for output in &outputs {
        layout.focus_output(output);
        for selected in 0..3 {
            layout.switch_workspace(selected);
            Op::CompleteAnimations.apply(&mut layout);
            let mon = layout.monitor_for_output(output).unwrap();
            let cells: Vec<_> = mon
                .workspaces_with_render_geo()
                .map(|(ws, geo)| (ws.id(), geo, mon.workspace_bounds(ws, geo)))
                .collect();
            for (id, geo, bounds) in &cells {
                let point = bounds.loc + bounds.size.to_point().downscale(2.);
                let (target, drop_geo) = mon.insert_position(point);
                assert!(matches!(target, InsertWorkspace::Existing(ws) if ws == *id));
                assert_eq!(drop_geo, *geo);
                if Rectangle::from_size(mon.workspaces[0].view_size()).contains(point) {
                    assert_eq!(mon.workspace_under(point).map(|(ws, _)| ws.id()), Some(*id));
                    assert_eq!(
                        mon.workspace_under_narrow(point).map(Workspace::id),
                        Some(*id)
                    );
                }
            }
            for pair in cells.windows(2) {
                let bottom = pair[0].2.loc.y + pair[0].2.size.h;
                let top = pair[1].2.loc.y;
                assert!(top > bottom, "allocated Sink cells overlap: {cells:?}");
                let point =
                    Point::from((pair[0].1.loc.x + pair[0].1.size.w / 2., (bottom + top) / 2.));
                let next_idx = mon
                    .workspaces
                    .iter()
                    .position(|ws| ws.id() == pair[1].0)
                    .unwrap();
                assert_eq!(
                    mon.insert_position(point).0,
                    InsertWorkspace::NewAt(next_idx)
                );
            }
            layout.verify_invariants();
        }
    }
}

#[test]
fn overview_tail_keeps_native_animation_live_only_while_visible() {
    let mut options = Options::default();
    options.sink.enabled = true;
    let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
    Op::AddOutput(1).apply(&mut layout);
    for id in 1..=4 {
        let mut params = TestWindowParams::new(id);
        if id == 1 {
            params.rules = Some(ResolvedWindowRules {
                baba_is_float: Some(true),
                ..Default::default()
            });
        }
        Op::AddWindow { params }.apply(&mut layout);
        settle_system_layout(&mut layout);
        assert!(layout.sink_window());
    }
    Op::CompleteAnimations.apply(&mut layout);
    assert!(!layout.active_workspace().unwrap().are_animations_ongoing());
    layout.open_overview();
    Op::CompleteAnimations.apply(&mut layout);
    assert!(layout.active_workspace().unwrap().are_animations_ongoing());
    layout.close_overview();
    Op::CompleteAnimations.apply(&mut layout);
    assert!(!layout.active_workspace().unwrap().are_animations_ongoing());
}

#[test]
fn workspace_lifo_unwind_and_arbitrary_close_preserve_other_owners() {
    let mut workspace = Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::default());
    for id in 1..=4 {
        add_window(&mut workspace, id, id % 2 == 0);
        assert!(workspace.sink_window(&id));
        settle(&mut workspace);
    }
    assert!(!workspace.sink_window(&4));
    assert!(!workspace.sink_window(&99));
    let removed = workspace.remove_tile(&2, Transaction::new());
    assert_eq!(removed.tile.window().id(), &2);
    assert!(removed.is_floating);
    settle(&mut workspace);
    assert_eq!(workspace.sink_depth(&4), Some(0));
    assert_eq!(workspace.sink_depth(&3), Some(1));
    assert_eq!(workspace.sink_depth(&1), Some(2));
    assert!(!workspace.unwind_sink_to(&99, true));
    assert!(workspace.unwind_sink_to(&3, true));
    settle(&mut workspace);
    assert_eq!(workspace.sink_count(), 1);
    assert_eq!(workspace.sink_depth(&1), Some(0));
    assert_eq!(workspace.active_window().map(LayoutElement::id), Some(&3));
    assert_eq!(workspace.windows().count(), 3);
    assert_eq!(workspace.pull_window(false), Some(1));
    settle(&mut workspace);
    assert_eq!(workspace.active_window().map(LayoutElement::id), Some(&3));
}

#[test]
fn workspace_restores_native_height_policy_after_reload_and_output_resize() {
    for height in [
        WindowHeight::Auto { weight: 9. },
        WindowHeight::Fixed(180.5),
        WindowHeight::Preset(0),
        WindowHeight::Preset(1),
    ] {
        let mut layout = Layout::<TestWindow>::default();
        Op::AddOutput(1).apply(&mut layout);
        let output = layout.outputs().next().unwrap().clone();
        let mut workspace = Workspace::new(
            output.clone(),
            Clock::with_time(Duration::ZERO),
            Rc::default(),
        );
        add_window(&mut workspace, 1, false);
        workspace.scrolling_mut().restore_window_height(&1, height);
        settle(&mut workspace);
        let saved = workspace.scrolling().window_height(&1).unwrap();
        assert!(workspace.sink_window(&1));
        let old_request = workspace.windows().next().unwrap().requested_size();
        let mut options = Options::default();
        options.sink.enabled = true;
        options.layout.preset_window_heights.truncate(1);
        workspace.update_config(Rc::new(options));
        output.change_current_state(
            Some(Mode {
                size: (1350, 900).into(),
                refresh: 60000,
            }),
            Some(Transform::Normal),
            Some(output::Scale::Fractional(1.5)),
            None,
        );
        workspace.update_output_size();
        settle(&mut workspace);
        assert_eq!(
            workspace.windows().next().unwrap().requested_size(),
            old_request
        );
        assert_eq!(workspace.pull_window(true), Some(1));
        settle(&mut workspace);
        let restored = workspace.scrolling().window_height(&1).unwrap();
        match saved {
            WindowHeight::Auto { .. } => assert_eq!(restored, WindowHeight::Auto { weight: 1. }),
            WindowHeight::Fixed(height) => assert_eq!(restored, WindowHeight::Fixed(height)),
            WindowHeight::Preset(0) => assert_eq!(restored, saved),
            WindowHeight::Preset(_) => assert_eq!(restored, WindowHeight::Auto { weight: 1. }),
        }
    }
}

#[test]
fn workspace_accepts_native_sizing_states_without_foreground_ownership() {
    for case in 0..6 {
        let mut workspace =
            Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::default());
        let mut win = add_window(&mut workspace, 1, false);
        match case {
            0 => workspace.set_fullscreen(&1, true),
            1 => workspace.set_maximized(&1, true),
            2 => {
                workspace.set_maximized(&1, true);
                workspace.set_fullscreen(&1, true);
            }
            3 => {
                workspace.set_fullscreen(&1, true);
                settle(&mut workspace);
                workspace.set_fullscreen(&1, false);
            }
            4 => win.request_windowed_fullscreen(true),
            _ => {
                win.request_windowed_fullscreen(true);
                settle(&mut workspace);
            }
        }
        let requested = win.requested_size();
        let mode = win.pending_sizing_mode();
        assert!(workspace.sink_window(&1));
        assert_eq!(workspace.sink_count(), 1);
        assert!(workspace.active_window().is_none());
        assert_eq!(win.requested_size(), requested);
        assert_eq!(win.pending_sizing_mode(), mode);
        assert_eq!(workspace.tiles_with_render_positions().count(), 0);
        assert!(!workspace.render_above_top_layer());
        settle(&mut workspace);
        assert_eq!(workspace.pull_window(true), Some(1));
        settle(&mut workspace);
        assert_eq!(win.pending_sizing_mode(), mode);
        if case == 2 {
            workspace.set_fullscreen(&1, false);
            assert_eq!(win.pending_sizing_mode(), SizingMode::Maximized);
        }
        workspace.verify_invariants(None);
    }
}

#[test]
fn sunk_native_requests_keep_background_role_and_floating_restore() {
    for floating in [false, true] {
        let mut workspace =
            Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::default());
        let win = add_window(&mut workspace, 1, floating);
        let native_size = win.size();
        add_window(&mut workspace, 2, false);
        workspace.set_maximized(&1, true);
        workspace.set_fullscreen(&1, true);
        settle(&mut workspace);
        assert!(workspace.sink_window(&1));
        settle(&mut workspace);
        assert_eq!(workspace.active_window().map(LayoutElement::id), Some(&2));
        assert!(!workspace.render_above_top_layer());
        assert!(!workspace.is_floating(&1));
        workspace.set_fullscreen(&1, false);
        assert_eq!(win.pending_sizing_mode(), SizingMode::Maximized);
        settle(&mut workspace);
        workspace.set_maximized(&1, false);
        assert_eq!(win.pending_sizing_mode(), SizingMode::Normal);
        settle(&mut workspace);
        assert_eq!(workspace.sink_depth(&1), Some(0));
        assert_eq!(workspace.active_window().map(LayoutElement::id), Some(&2));
        assert_eq!(workspace.pull_window(false), Some(1));
        settle(&mut workspace);
        assert_eq!(workspace.is_floating(&1), floating);
        if floating {
            assert_eq!(win.size(), native_size);
        }
        assert_eq!(workspace.active_window().map(LayoutElement::id), Some(&2));
        workspace.verify_invariants(None);
    }
}

#[test]
fn sunk_fullscreen_request_does_not_revive_a_stale_floating_origin() {
    let mut workspace = Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::default());
    let win = add_window(&mut workspace, 1, true);
    workspace.toggle_window_floating(Some(&1));
    settle(&mut workspace);
    assert!(!workspace.is_floating(&1));
    assert!(workspace.sink_window(&1));
    workspace.set_fullscreen(&1, true);
    settle(&mut workspace);
    assert_eq!(workspace.pull_window(true), Some(1));
    settle(&mut workspace);
    workspace.set_fullscreen(&1, false);
    settle(&mut workspace);
    assert!(!workspace.is_floating(&1));
    assert_eq!(win.pending_sizing_mode(), SizingMode::Normal);
    workspace.verify_invariants(None);
}

#[test]
fn sunk_direct_layout_edits_are_noops_and_child_insertion_is_normal() {
    let mut workspace = Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::default());
    let win = add_window(&mut workspace, 1, false);
    add_window(&mut workspace, 2, false);
    assert!(workspace.sink_window(&1));
    let requested = win.requested_size();
    workspace.set_window_width(Some(&1), SizeChange::SetFixed(700));
    workspace.set_window_height(Some(&1), SizeChange::SetFixed(100));
    workspace.reset_window_height(Some(&1));
    workspace.toggle_window_width(Some(&1), true);
    workspace.toggle_window_height(Some(&1), true);
    workspace.set_window_floating(Some(&1), true);
    workspace.toggle_window_floating(Some(&1));
    workspace.center_window(Some(&1));
    workspace.consume_or_expel_window_left(Some(&1));
    workspace.consume_or_expel_window_right(Some(&1));
    workspace.move_floating_window(
        Some(&1),
        PositionChange::SetFixed(10.),
        PositionChange::SetFixed(10.),
        false,
    );
    workspace.toggle_fullscreen(&1);
    workspace.toggle_maximized(&1);
    assert!(!workspace.interactive_resize_begin(1, ResizeEdge::RIGHT));
    assert!(!workspace.interactive_resize_update(&1, (20., 0.).into()));
    workspace.interactive_resize_end(Some(&1));
    assert_eq!(win.requested_size(), requested);
    assert_eq!(workspace.sink_depth(&1), Some(0));
    let child = TestWindow::new(TestWindowParams::new(3));
    workspace.add_tile(
        workspace.make_tile(child),
        WorkspaceAddWindowTarget::NextTo(&1),
        ActivateWindow::Smart,
        ColumnWidth::Fixed(300.),
        false,
        true,
        None,
    );
    settle(&mut workspace);
    assert_eq!(workspace.sink_depth(&1), Some(0));
    assert_eq!(workspace.windows().count(), 3);
    assert_eq!(workspace.active_window().map(LayoutElement::id), Some(&2));
}

#[test]
fn sunk_owners_receive_real_output_changes_and_close_notifications() {
    let mut layout = Layout::<TestWindow>::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddOutput(2).apply(&mut layout);
    let output1 = layout
        .outputs()
        .find(|o| o.name() == "output1")
        .unwrap()
        .clone();
    let output2 = layout
        .outputs()
        .find(|o| o.name() == "output2")
        .unwrap()
        .clone();
    let mut workspace = Workspace::new(output1, Clock::with_time(Duration::ZERO), Rc::default());
    let first = add_window(&mut workspace, 1, true);
    let second = add_window(&mut workspace, 2, false);
    assert!(workspace.sink_window(&1));
    assert!(workspace.sink_window(&2));
    first.0.output_events.borrow_mut().clear();
    second.0.output_events.borrow_mut().clear();
    workspace.set_output(None);
    workspace.set_output(Some(output2));
    settle(&mut workspace);
    for win in [&first, &second] {
        assert_eq!(
            win.0.output_events.take(),
            [("output1".to_owned(), false), ("output2".to_owned(), true)]
        );
    }
    let removed = workspace.remove_tile(&1, Transaction::new());
    assert!(removed.is_floating);
    assert!(Rc::ptr_eq(&removed.tile.window().0, &first.0));
    assert_eq!(
        first.0.output_events.take(),
        [("output2".to_owned(), false)]
    );
    assert!(second.0.output_events.borrow().is_empty());
    assert_eq!(workspace.sink_depth(&2), Some(0));
    assert_eq!(workspace.pull_window(true), Some(2));
    settle(&mut workspace);
    assert!(second.0.output_events.borrow().is_empty());
}

#[test]
fn sunk_client_commit_updates_native_geometry_without_depth_resize() {
    let mut workspace = Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::default());
    let window = add_window(&mut workspace, 1, true);
    assert!(workspace.sink_window(&1));
    let requested = window.requested_size();
    window.0.forced_size.set(Some((345, 234).into()));
    assert!(window.communicate());
    workspace.update_window(&1, Some(Serial::from(7)));
    workspace.verify_invariants(None);
    let (_, ipc) = workspace.tiles_with_ipc_layouts().next().unwrap();
    assert_eq!(ipc.window_size, (345, 234));
    assert_eq!(window.requested_size(), requested);
    assert_eq!(workspace.sink_depth(&1), Some(0));
    assert_eq!(workspace.pull_window(false), Some(1));
    settle(&mut workspace);
    assert!(Rc::ptr_eq(
        &window.0,
        &workspace.windows().next().unwrap().0
    ));
}

#[test]
fn layout_keeps_sink_workspace_through_output_disconnect_reconnect_and_close() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    Op::Communicate(1).apply(&mut layout);
    Op::Refresh { is_active: true }.apply(&mut layout);
    let win = layout.windows().next().unwrap().1.clone();
    let workspace_id = layout.active_workspace().unwrap().id();
    assert!(layout.sink_window());
    layout.verify_invariants();
    Op::Refresh { is_active: true }.apply(&mut layout);
    assert_eq!(layout.sink_depth(&1), Some(0));
    assert!(layout.active_workspace().unwrap().has_windows());
    assert!(layout.focus_with_output().is_none());
    assert!(!layout.should_trigger_focus_follows_mouse_on(&1));
    let output = layout.outputs().next().unwrap().clone();
    assert!(!layout.interactive_move_begin(1, &output, (10., 10.).into()));
    layout.toggle_windowed_fullscreen(&1);
    assert!(!win.is_pending_windowed_fullscreen());
    win.0.output_events.borrow_mut().clear();
    Op::RemoveOutput(1).apply(&mut layout);
    layout.verify_invariants();
    assert!(layout.has_window(&1));
    assert_eq!(
        layout
            .find_workspace_by_id(workspace_id)
            .unwrap()
            .1
            .sink_count(),
        1
    );
    Op::AddOutput(2).apply(&mut layout);
    layout.verify_invariants();
    assert_eq!(
        layout
            .find_workspace_by_id(workspace_id)
            .unwrap()
            .1
            .sink_count(),
        1
    );
    assert_eq!(
        win.0.output_events.take(),
        [("output1".to_owned(), false), ("output2".to_owned(), true)]
    );
    Op::CloseWindow(1).apply(&mut layout);
    layout.verify_invariants();
    assert!(!layout.has_window(&1));
    assert_eq!(win.0.output_events.take(), [("output2".to_owned(), false)]);
}

#[test]
fn layout_explicit_recall_is_distinct_from_generic_activation() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    for id in 1..=3 {
        Op::AddWindow {
            params: TestWindowParams::new(id),
        }
        .apply(&mut layout);
        Op::Communicate(id).apply(&mut layout);
        assert!(layout.sink_window());
        layout.verify_invariants();
    }
    layout.activate_window(&1);
    layout.activate_window_without_raising(&1);
    assert_eq!(layout.sink_depth(&1), Some(2));
    assert!(layout.focus_with_output().is_none());
    let mut layouts = Vec::new();
    layout.with_windows(|window, _, workspace, ipc| {
        layouts.push((*window.id(), workspace, ipc));
    });
    assert_eq!(layouts.len(), 3);
    assert!(layouts
        .iter()
        .all(|(_, ws, ipc)| ws.is_some() && ipc.pos_in_scrolling_layout.is_none()));
    Op::CloseWindow(2).apply(&mut layout);
    assert_eq!(layout.sink_depth(&1), Some(1));
    assert!(layout.unwind_sink_to(&1));
    layout.activate_window(&1);
    layout.verify_invariants();
    assert_eq!(
        layout.focus_with_output().map(|(win, _)| *win.id()),
        Some(1)
    );
    assert_eq!(layout.sink_depth(&1), None);
    assert_eq!(layout.sink_depth(&3), None);
}

#[test]
fn layout_direct_moves_of_sunk_owners_are_noops() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddOutput(2).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    assert!(layout.sink_window());
    let workspace = layout
        .workspaces()
        .find(|(_, _, ws)| ws.has_window(&1))
        .unwrap()
        .2
        .id();
    let output = layout
        .outputs()
        .find(|o| o.name() == "output2")
        .unwrap()
        .clone();
    layout.move_to_workspace(Some(&1), 1, ActivateWindow::Yes);
    layout.move_to_output(Some(&1), &output, None, ActivateWindow::Yes);
    layout.verify_invariants();
    assert_eq!(
        layout
            .find_workspace_by_id(workspace)
            .unwrap()
            .1
            .sink_depth(&1),
        Some(0)
    );
    assert_eq!(layout.windows().count(), 1);
}

#[test]
fn layout_rejects_active_transient_family_without_partial_transfer() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    let mut child = TestWindowParams::new(2);
    child.parent_id = Some(1);
    child.is_floating = true;
    Op::AddWindow { params: child }.apply(&mut layout);
    assert!(!layout.sink_window());
    assert_eq!(layout.sink_depth(&2), None);
    layout.activate_window(&1);
    assert!(!layout.sink_window());
    assert_eq!(layout.sink_depth(&1), None);
    assert_eq!(layout.windows().count(), 2);
    layout.verify_invariants();
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: if std::env::var_os("RUN_SLOW_TESTS").is_some() { ProptestConfig::default().cases } else { 0 },
        ..ProptestConfig::default()
    })]

    #[test]
    fn random_operations_from_sink_stack_dont_panic(ops: Vec<Op>, layout_config in arbitrary_layout_part()) {
        let options = Options { layout: niri_config::Layout::from_part(&layout_config), ..Default::default() };
        let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
        Op::AddOutput(1).apply(&mut layout);
        Op::AddOutput(2).apply(&mut layout);
        for id in 1..=3 {
            let mut params = TestWindowParams::new(id);
            params.is_floating = id == 2;
            Op::AddWindow { params }.apply(&mut layout);
            Op::Communicate(id).apply(&mut layout);
            prop_assert!(layout.sink_window());
            layout.verify_invariants();
        }
        for op in ops {
            op.apply(&mut layout);
            layout.verify_invariants();
        }
    }
}

#[test]
fn disabling_sink_restores_every_workspace_including_without_outputs() {
    for no_outputs in [false, true] {
        let mut layout = Layout::default();
        Op::AddOutput(1).apply(&mut layout);
        Op::SetSinkEnabled(true).apply(&mut layout);
        for id in 1..=3 {
            Op::AddWindow {
                params: TestWindowParams::new(id),
            }
            .apply(&mut layout);
            Op::Communicate(id).apply(&mut layout);
            assert!(layout.sink_window());
        }
        let owners = layout.windows().map(|(_, w)| w.clone()).collect::<Vec<_>>();
        if no_outputs {
            Op::RemoveOutput(1).apply(&mut layout);
        }
        for owner in &owners {
            owner.0.output_events.borrow_mut().clear();
        }
        Op::SetSinkEnabled(false).apply(&mut layout);
        layout.verify_invariants();
        assert_eq!(layout.windows().count(), 3);
        for owner in &owners {
            assert_eq!(layout.sink_depth(owner.id()), None);
            assert!(owner.0.output_events.borrow().is_empty());
            let (_, restored) = layout
                .windows()
                .find(|(_, w)| w.id() == owner.id())
                .unwrap();
            assert!(Rc::ptr_eq(&owner.0, &restored.0));
        }
    }
}

#[test]
fn enabled_sink_projection_reverses_current_pose_and_has_one_owner() {
    let mut layout = Layout::<TestWindow>::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::SetSinkEnabled(true).apply(&mut layout);
    let output = layout.outputs().next().unwrap().clone();
    let mut clock = Clock::with_time(Duration::ZERO);
    let options = layout.options.clone();
    let mut workspace = Workspace::new(output, clock.clone(), options);
    let owner = add_window(&mut workspace, 1, false);
    let request = owner.requested_size();
    assert!(workspace.sink_window(&1));
    let from = workspace
        .tiles()
        .find(|tile| tile.window().id() == &1)
        .unwrap()
        .sink_presentation
        .as_ref()
        .unwrap()
        .pose();
    assert_eq!(owner.requested_size(), request);
    assert_eq!(workspace.pull_window(true), Some(1));
    let tile = workspace.tiles().next().unwrap();
    assert!(Rc::ptr_eq(&tile.window().0, &owner.0));
    assert_eq!(tile.sink_presentation.as_ref().unwrap().pose(), from);
    assert!(workspace.active_window().is_none());
    assert!(workspace.sink_window(&1));
    assert_eq!(
        workspace
            .tiles()
            .find(|tile| tile.window().id() == &1)
            .unwrap()
            .sink_presentation
            .as_ref()
            .unwrap()
            .pose(),
        from
    );
    assert_eq!(workspace.windows().count(), 1);
    assert_eq!(workspace.tiles_with_render_positions().count(), 0);
    clock.set_complete_instantly(true);
    workspace.advance_animations();
    clock.set_complete_instantly(false);
    let final_pose = workspace
        .tiles()
        .find(|tile| tile.window().id() == &1)
        .unwrap()
        .sink_presentation
        .as_ref()
        .unwrap()
        .pose();
    assert!(final_pose.scale < 1.);
    assert!(final_pose.alpha < 1.);
    assert!(workspace
        .tiles()
        .find(|tile| tile.window().id() == &1)
        .unwrap()
        .sink_presentation
        .as_ref()
        .unwrap()
        .is_done());
}

#[test]
fn restore_pending_floating_role_change_preserves_selected_container() {
    for state in [
        Op::MaximizeWindowToEdges { id: Some(5) },
        Op::FullscreenWindow(5),
    ] {
        let mut params = TestWindowParams::new(5);
        params.is_floating = true;
        let layout = check_ops([
            Op::SetSinkEnabled(true),
            Op::AddOutput(1),
            Op::AddWindow { params },
            Op::SinkWindow,
            Op::PullWindow,
            state,
        ]);
        assert!(layout
            .active_workspace()
            .unwrap()
            .scrolling()
            .tiles()
            .any(|tile| tile.window().id() == &5));
        assert_eq!(layout.windows().count(), 1);
    }
}

fn settle_system_layout(layout: &mut Layout<TestWindow>) {
    for _ in 0..3 {
        let ids = layout
            .windows()
            .map(|(_, win)| *win.id())
            .collect::<Vec<_>>();
        for id in ids {
            Op::Communicate(id).apply(layout);
        }
        Op::Refresh { is_active: true }.apply(layout);
        layout.verify_invariants();
    }
}

#[test]
fn system_independent_stacks_survive_workspace_moves_all_outputs_removed_and_reordered_reconnect() {
    for remove_order in [[1, 2, 3], [3, 1, 2]] {
        let mut options = Options::default();
        options.sink.enabled = true;
        let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
        for id in 1..=3 {
            Op::AddOutput(id).apply(&mut layout);
        }
        let outputs = (1..=3)
            .map(|id| {
                layout
                    .outputs()
                    .find(|o| o.name() == format!("output{id}"))
                    .unwrap()
                    .clone()
            })
            .collect::<Vec<_>>();
        outputs[1].change_current_state(
            None,
            Some(Transform::_90),
            Some(output::Scale::Fractional(1.25)),
            None,
        );
        outputs[2].change_current_state(
            None,
            Some(Transform::_270),
            Some(output::Scale::Fractional(1.5)),
            None,
        );
        layout.update_output_size(&outputs[1]);
        layout.update_output_size(&outputs[2]);
        let mut workspace_ids = Vec::new();
        let mut owners = Vec::new();
        for (group, output) in outputs[..2].iter().enumerate() {
            layout.focus_output(output);
            for id in (group * 3 + 1)..=(group * 3 + 3) {
                let mut params = TestWindowParams::new(id);
                params.is_floating = id % 2 == 0;
                Op::AddWindow { params }.apply(&mut layout);
                settle_system_layout(&mut layout);
                owners.push(
                    layout
                        .windows()
                        .find(|(_, w)| *w.id() == id)
                        .unwrap()
                        .1
                        .clone(),
                );
                assert!(layout.sink_window());
            }
            workspace_ids.push(layout.active_workspace().unwrap().id());
        }
        layout.focus_output(&outputs[0]);
        assert!(layout.move_workspace_to_output(&outputs[2]));
        layout.move_workspace_to_idx(None, 0);
        settle_system_layout(&mut layout);
        for id in remove_order {
            Op::RemoveOutput(id).apply(&mut layout);
            settle_system_layout(&mut layout);
        }
        assert_eq!(layout.outputs().count(), 0);
        assert_eq!(layout.windows().count(), 6);
        for (group, workspace_id) in workspace_ids.iter().enumerate() {
            let ws = layout.find_workspace_by_id(*workspace_id).unwrap().1;
            for depth in 0..3 {
                assert_eq!(ws.sink_depth(&(group * 3 + 3 - depth)), Some(depth));
            }
        }
        // Close a middle entry without an output; the other workspace must not be affected.
        Op::CloseWindow(2).apply(&mut layout);
        for id in [2, 3, 1] {
            Op::AddOutput(id).apply(&mut layout);
            settle_system_layout(&mut layout);
        }
        for owner in owners.iter().filter(|w| *w.id() != 2) {
            let current = layout
                .windows()
                .find(|(_, w)| w.id() == owner.id())
                .unwrap()
                .1;
            assert!(Rc::ptr_eq(&current.0, &owner.0));
        }
        assert_eq!(
            layout
                .find_workspace_by_id(workspace_ids[0])
                .unwrap()
                .1
                .sink_count(),
            2
        );
        assert_eq!(
            layout
                .find_workspace_by_id(workspace_ids[1])
                .unwrap()
                .1
                .sink_count(),
            3
        );
        assert!(layout.unwind_sink_to(&4));
        settle_system_layout(&mut layout);
        assert_eq!(layout.sink_depth(&1), Some(1));
        assert_eq!(layout.sink_depth(&3), Some(0));
        assert!(layout
            .windows()
            .filter(|(_, w)| *w.id() >= 4)
            .all(|(_, w)| layout.sink_depth(w.id()).is_none()));
        assert!(layout.unwind_sink_to(&1));
        settle_system_layout(&mut layout);
        assert_eq!(layout.windows().count(), 5);
        assert!(layout.workspaces().all(|(_, _, ws)| ws.sink_count() == 0));
    }
}

#[test]
fn system_feature_disable_without_outputs_restores_multiple_stacks_in_place() {
    let mut options = Options::default();
    options.sink.enabled = true;
    let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
    for out in 1..=2 {
        Op::AddOutput(out).apply(&mut layout);
        let output = layout
            .outputs()
            .find(|o| o.name() == format!("output{out}"))
            .unwrap()
            .clone();
        layout.focus_output(&output);
        for id in (out * 3 - 2)..=(out * 3) {
            Op::AddWindow {
                params: TestWindowParams::new(id),
            }
            .apply(&mut layout);
            settle_system_layout(&mut layout);
            assert!(layout.sink_window());
        }
    }
    let workspace_ids = layout
        .workspaces()
        .filter(|(_, _, ws)| ws.sink_count() > 0)
        .map(|(_, _, ws)| ws.id())
        .collect::<Vec<_>>();
    Op::RemoveOutput(1).apply(&mut layout);
    Op::RemoveOutput(2).apply(&mut layout);
    Op::SetSinkEnabled(false).apply(&mut layout);
    settle_system_layout(&mut layout);
    assert_eq!(layout.windows().count(), 6);
    for id in workspace_ids {
        let ws = layout.find_workspace_by_id(id).unwrap().1;
        assert_eq!(ws.sink_count(), 0);
        assert_eq!(ws.windows().count(), 3);
    }
    Op::AddOutput(2).apply(&mut layout);
    settle_system_layout(&mut layout);
    assert_eq!(layout.windows().count(), 6);
}
