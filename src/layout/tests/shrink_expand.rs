use super::*;
use crate::layout::floating::FloatingSpace;
use crate::layout::scrolling::ScrollingSpace;
use crate::layout::shrink_expand::{ShrinkAxis, SpineGeometry, WindowForm};
use crate::layout::workspace::{Workspace, WorkspaceAddWindowTarget};

#[test]
fn moving_spine_mode_intent_waits_for_native_drop_and_toggles_virtual_flags() {
    for scale in [1., 1.25] {
        for floating in [false, true] {
            for starting in [false, true] {
                for case in [
                    "full",
                    "max",
                    "latent",
                    "windowed",
                    "cancel-full",
                    "cancel-windowed",
                ] {
                    let mut options = Options::default();
                    options.animations.off = true;
                    let mut layout =
                        Layout::with_options(Clock::with_time(Duration::ZERO), options);
                    layout.clock.set_complete_instantly(true);
                    Op::AddScaledOutput {
                        id: 1,
                        scale,
                        layout_config: None,
                    }
                    .apply(&mut layout);
                    Op::AddWindow {
                        params: TestWindowParams::new(1),
                    }
                    .apply(&mut layout);
                    Op::Communicate(1).apply(&mut layout);
                    if floating {
                        layout.set_window_floating(Some(&1), true);
                    }
                    Op::Communicate(1).apply(&mut layout);
                    assert!(layout.commit_shrink(&1, ShrinkAxis::Horizontal, 32.));
                    let client = layout.windows().next().unwrap().1.clone();
                    let request = client.requested_size();
                    layout.advance_animations();
                    let output = layout.outputs().next().unwrap().clone();
                    assert!(layout.interactive_move_begin(1, &output, (10., 100.).into()));
                    // The first request can arrive before native detachment.
                    layout.request_toggle_fullscreen(&1);
                    if !starting {
                        layout.interactive_move_update(
                            &1,
                            (350., 0.).into(),
                            output,
                            (500., 300.).into(),
                        );
                        assert!(matches!(
                            layout.interactive_move,
                            Some(InteractiveMoveState::Moving(_))
                        ));
                    }
                    match case {
                        "full" => (),
                        "max" => {
                            layout.request_fullscreen(&1, false);
                            layout.request_maximized(&1, true);
                        }
                        "latent" => layout.request_maximized(&1, true),
                        "windowed" => {
                            layout.request_maximized(&1, true);
                            layout.request_toggle_windowed_fullscreen(&1);
                        }
                        "cancel-full" => layout.request_toggle_fullscreen(&1),
                        "cancel-windowed" => {
                            layout.request_toggle_windowed_fullscreen(&1);
                            layout.request_fullscreen(&1, false);
                        }
                        _ => unreachable!(),
                    }
                    assert!(layout.is_spine(&1));
                    assert_eq!(client.requested_size(), request);
                    assert!(client.pending_sizing_mode().is_normal());
                    assert!(!client.is_pending_windowed_fullscreen());
                    layout.verify_invariants();
                    layout.interactive_move_end(&1);
                    assert!(layout.moving_spine_mode.is_none());
                    assert!(!layout.is_spine(&1));
                    let expected = match case {
                        "full" | "latent" => SizingMode::Fullscreen,
                        "max" | "windowed" => SizingMode::Maximized,
                        _ => SizingMode::Normal,
                    };
                    assert_eq!(client.pending_sizing_mode(), expected, "{case}");
                    assert_eq!(client.is_pending_windowed_fullscreen(), case == "windowed");
                    if case == "latent" {
                        layout.request_fullscreen(&1, false);
                        assert_eq!(client.pending_sizing_mode(), SizingMode::Maximized);
                    }
                    assert!(Rc::ptr_eq(
                        &layout.windows().next().unwrap().1 .0,
                        &client.0
                    ));
                    layout.verify_invariants();
                }
            }
        }
    }
}

fn shrink_mode_matrix_layout(scale: f64, empty_above: bool) -> Layout<TestWindow> {
    let mut options = Options::default();
    options.animations.off = true;
    options.layout.empty_workspace_above_first = empty_above;
    let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
    layout.clock.set_complete_instantly(true);
    Op::AddScaledOutput {
        id: 1,
        scale,
        layout_config: None,
    }
    .apply(&mut layout);
    for id in [1, 2] {
        Op::AddWindow {
            params: TestWindowParams::new(id),
        }
        .apply(&mut layout);
        Op::Communicate(id).apply(&mut layout);
    }
    layout
}

fn request_moving_spine_test_mode(layout: &mut Layout<TestWindow>, mode: &str) {
    match mode {
        "full" => layout.request_fullscreen(&1, true),
        "max" => layout.request_maximized(&1, true),
        "latent" => {
            layout.request_fullscreen(&1, true);
            layout.request_maximized(&1, true);
        }
        "windowed" => {
            layout.request_maximized(&1, true);
            layout.request_toggle_windowed_fullscreen(&1);
        }
        "cancel" => {
            layout.request_toggle_fullscreen(&1);
            layout.request_toggle_fullscreen(&1);
        }
        _ => unreachable!(),
    }
}

#[test]
fn shrink_moving_mode_final_shared_and_tabbed_slot_restores_only_required_members() {
    for scale in [1., 1.25] {
        for display in [ColumnDisplay::Normal, ColumnDisplay::Tabbed] {
            for destination in [
                SizingMode::Normal,
                SizingMode::Fullscreen,
                SizingMode::Maximized,
            ] {
                for mode in ["full", "max", "latent", "windowed", "cancel"] {
                    let mut layout = shrink_mode_matrix_layout(scale, false);
                    Op::AddWindow {
                        params: TestWindowParams::new(3),
                    }
                    .apply(&mut layout);
                    Op::Communicate(3).apply(&mut layout);
                    layout.consume_or_expel_window_left(Some(&3));
                    layout.set_column_display(display);
                    Op::Communicate(2).apply(&mut layout);
                    Op::Communicate(3).apply(&mut layout);
                    assert!(layout.commit_shrink(&3, ShrinkAxis::Horizontal, 32.));
                    match destination {
                        SizingMode::Fullscreen => layout.request_fullscreen(&2, true),
                        SizingMode::Maximized => layout.request_maximized(&2, true),
                        SizingMode::Normal => (),
                    }
                    Op::Communicate(2).apply(&mut layout);
                    Op::Communicate(3).apply(&mut layout);
                    assert!(layout.commit_shrink(&1, ShrinkAxis::Vertical, 40.));
                    let client = layout
                        .windows()
                        .find(|(_, w)| w.id() == &1)
                        .unwrap()
                        .1
                        .clone();
                    let request = client.requested_size();
                    let peer_book = layout.is_spine(&3);
                    let output = layout.outputs().next().unwrap().clone();
                    layout.advance_animations();
                    assert!(layout.interactive_move_begin(1, &output, (60., 30.).into()));
                    assert!(layout.interactive_move_update(
                        &1,
                        (300., 0.).into(),
                        output.clone(),
                        (500., 250.).into()
                    ));
                    request_moving_spine_test_mode(&mut layout, mode);
                    assert!(layout.is_spine(&1));
                    assert_eq!(client.requested_size(), request);
                    assert_eq!(
                        layout.is_spine(&3),
                        peer_book,
                        "intent must not change destination before drop"
                    );
                    layout.advance_animations();
                    let ws = layout.active_workspace().unwrap();
                    let columns_before = ws.scrolling().columns().count();
                    let column = ws
                        .scrolling()
                        .columns()
                        .position(|col| col.tiles().any(|(t, _)| t.window().id() == &2))
                        .unwrap();
                    let size = output_size(&output);
                    let point = (0..size.h as i32).step_by(10).find_map(|y| {
                        (0..size.w as i32).step_by(10).find_map(|x| {
                            let p = Point::from((f64::from(x), f64::from(y)));
                            matches!(ws.scrolling_insert_position(p), InsertPosition::InColumn(i, _) if i == column).then_some(p)
                        })
                    }).expect("real shared-column gap");
                    assert!(layout.interactive_move_update(&1, (0., 0.).into(), output, point));
                    layout.update_render_elements(None);
                    assert!(
                        matches!(layout.monitors().next().unwrap().insert_hint.as_ref().unwrap().position, InsertPosition::InColumn(i, _) if i == column)
                    );
                    layout.verify_invariants();
                    layout.interactive_move_end(&1);
                    assert!(layout.moving_spine_mode.is_none());
                    assert!(!layout.is_spine(&1));
                    let expected = match mode {
                        "full" | "latent" => SizingMode::Fullscreen,
                        "max" | "windowed" => SizingMode::Maximized,
                        _ => SizingMode::Normal,
                    };
                    assert_eq!(
                        client.pending_sizing_mode(),
                        expected,
                        "{display:?}/{destination:?}/{mode}"
                    );
                    assert_eq!(client.is_pending_windowed_fullscreen(), mode == "windowed");
                    assert_eq!(
                        layout.is_spine(&3),
                        peer_book && (display == ColumnDisplay::Normal || mode == "cancel"),
                        "native column scope: {display:?}/{destination:?}/{mode}"
                    );
                    let ws = layout
                        .workspaces()
                        .find(|(_, _, ws)| ws.has_window(&1))
                        .unwrap()
                        .2;
                    assert!(ws.has_window(&2) && ws.has_window(&3));
                    assert_eq!(
                        ws.scrolling().columns().count(),
                        columns_before
                            + usize::from(display == ColumnDisplay::Normal && mode != "cancel"),
                        "native mode extraction: {display:?}/{destination:?}/{mode}"
                    );
                    assert!(Rc::ptr_eq(
                        &ws.tiles()
                            .find(|t| t.window().id() == &1)
                            .unwrap()
                            .window()
                            .0,
                        &client.0
                    ));
                    if mode == "latent" {
                        layout.request_fullscreen(&1, false);
                        assert_eq!(client.pending_sizing_mode(), SizingMode::Maximized);
                    }
                    for id in [1, 2, 3] {
                        Op::Communicate(id).apply(&mut layout);
                    }
                    layout.refresh(true);
                    layout.verify_invariants();
                }
            }
        }
    }
}

#[test]
fn shrink_moving_mode_output_loss_and_live_config_use_final_owner() {
    for floating in [false, true] {
        for case in ["cross", "source-lost", "target-lost", "all-lost", "reload"] {
            for mode in ["full", "max", "latent", "windowed", "cancel"] {
                let mut layout = shrink_mode_matrix_layout(1., false);
                Op::AddScaledOutput {
                    id: 2,
                    scale: 1.25,
                    layout_config: None,
                }
                .apply(&mut layout);
                if floating {
                    layout.set_window_floating(Some(&1), true);
                }
                Op::Communicate(1).apply(&mut layout);
                assert!(layout.commit_shrink(&1, ShrinkAxis::Vertical, 40.));
                let client = layout
                    .windows()
                    .find(|(_, w)| w.id() == &1)
                    .unwrap()
                    .1
                    .clone();
                let request = client.requested_size();
                let source = layout
                    .outputs()
                    .find(|o| o.name() == "output1")
                    .unwrap()
                    .clone();
                let target = layout
                    .outputs()
                    .find(|o| o.name() == "output2")
                    .unwrap()
                    .clone();
                assert!(layout.interactive_move_begin(1, &source, (60., 30.).into()));
                assert!(layout.interactive_move_update(
                    &1,
                    (300., 0.).into(),
                    target.clone(),
                    (400., 250.).into()
                ));
                request_moving_spine_test_mode(&mut layout, mode);
                if case == "reload" {
                    let mut config = niri_config::Config::default();
                    config.animations.off = true;
                    config.shrink_expand.enabled = true;
                    config.shrink_expand.thickness = 48.;
                    config.layout.border.off = false;
                    config.layout.border.width = 7.;
                    layout.update_config(&config);
                }
                if case == "source-lost" || case == "all-lost" {
                    layout.remove_output(&source);
                }
                if case == "target-lost" || case == "all-lost" {
                    layout.remove_output(&target);
                }
                assert!(layout.is_spine(&1));
                assert_eq!(client.requested_size(), request);
                layout.verify_invariants();
                layout.interactive_move_end(&1);
                assert!(layout.interactive_move.is_none() && layout.moving_spine_mode.is_none());
                assert!(!layout.is_spine(&1));
                let expected = match mode {
                    "full" | "latent" => SizingMode::Fullscreen,
                    "max" | "windowed" => SizingMode::Maximized,
                    _ => SizingMode::Normal,
                };
                assert_eq!(
                    client.pending_sizing_mode(),
                    expected,
                    "{floating}/{case}/{mode}"
                );
                assert_eq!(client.is_pending_windowed_fullscreen(), mode == "windowed");
                let (output, _, ws) = layout
                    .workspaces()
                    .find(|(_, _, ws)| ws.has_window(&1))
                    .unwrap();
                assert!(Rc::ptr_eq(
                    &ws.tiles()
                        .find(|t| t.window().id() == &1)
                        .unwrap()
                        .window()
                        .0,
                    &client.0
                ));
                match case {
                    "target-lost" => assert_eq!(output.map(|m| &m.output), Some(&source)),
                    "all-lost" => assert!(output.is_none()),
                    _ => assert_eq!(output.map(|m| &m.output), Some(&target)),
                }
                if mode == "latent" {
                    layout.request_fullscreen(&1, false);
                    assert_eq!(client.pending_sizing_mode(), SizingMode::Maximized);
                }
                Op::Communicate(1).apply(&mut layout);
                layout.refresh(true);
                layout.verify_invariants();
            }
        }
    }
}

#[test]
fn shrink_moving_mode_overview_gap_keeps_new_workspace_identity() {
    for scale in [1., 1.25] {
        for gap in ["top", "middle", "bottom"] {
            for mode in ["full", "max", "windowed", "cancel"] {
                let mut layout = shrink_mode_matrix_layout(scale, gap == "top");
                let source_idx = usize::from(gap == "top");
                layout.move_to_workspace(Some(&2), source_idx + 1, ActivateWindow::No);
                assert!(layout.commit_shrink(&1, ShrinkAxis::Vertical, 40.));
                let client = layout
                    .windows()
                    .find(|(_, w)| w.id() == &1)
                    .unwrap()
                    .1
                    .clone();
                let request = client.requested_size();
                let output = layout.outputs().next().unwrap().clone();
                assert!(layout.interactive_move_begin(1, &output, (60., 30.).into()));
                assert!(layout.interactive_move_update(
                    &1,
                    (300., 0.).into(),
                    output.clone(),
                    (500., 250.).into()
                ));
                request_moving_spine_test_mode(&mut layout, mode);
                layout.open_overview();
                if gap == "top" {
                    layout.switch_workspace(0);
                }
                if gap == "bottom" {
                    layout.switch_workspace(source_idx + 1);
                }
                layout.clock.set_unadjusted(Duration::from_millis(1));
                layout.advance_animations();
                let mon = layout.monitors().next().unwrap();
                let desired = match gap {
                    "top" => 0,
                    "middle" => source_idx + 1,
                    _ => mon.workspaces.len() - 1,
                };
                let size = output_size(&output);
                let point = (0..size.h as i32)
                    .find_map(|y| {
                        let p = Point::from((size.w / 2., f64::from(y)));
                        (mon.insert_position(p).0 == InsertWorkspace::NewAt(desired)).then_some(p)
                    })
                    .expect("visible native overview workspace gap");
                let old_ids = mon.workspaces.iter().map(|ws| ws.id()).collect::<Vec<_>>();
                assert!(layout.interactive_move_update(&1, (0., 0.).into(), output, point));
                layout.update_render_elements(None);
                assert_eq!(
                    layout
                        .monitors()
                        .next()
                        .unwrap()
                        .insert_hint
                        .as_ref()
                        .unwrap()
                        .workspace,
                    InsertWorkspace::NewAt(desired)
                );
                assert!(layout.is_spine(&1));
                assert_eq!(client.requested_size(), request);
                layout.interactive_move_end(&1);
                let ws = layout
                    .workspaces()
                    .find(|(_, _, ws)| ws.has_window(&1))
                    .unwrap()
                    .2;
                assert_eq!(!old_ids.contains(&ws.id()), gap == "middle");
                assert!(!layout.is_spine(&1));
                assert!(layout.moving_spine_mode.is_none());
                assert!(Rc::ptr_eq(
                    &ws.tiles().next().unwrap().window().0,
                    &client.0
                ));
                assert!(layout.is_overview_open());
                let expected = match mode {
                    "full" => SizingMode::Fullscreen,
                    "max" | "windowed" => SizingMode::Maximized,
                    _ => SizingMode::Normal,
                };
                assert_eq!(client.pending_sizing_mode(), expected);
                Op::Communicate(1).apply(&mut layout);
                layout.verify_invariants();
            }
        }
    }
}

#[test]
fn moving_spine_mode_close_removes_intent_and_normal_move_keeps_native_policy() {
    for spine in [false, true] {
        let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), Options::default());
        Op::AddOutput(1).apply(&mut layout);
        Op::AddWindow {
            params: TestWindowParams::new(1),
        }
        .apply(&mut layout);
        Op::Communicate(1).apply(&mut layout);
        if spine {
            assert!(layout.commit_shrink(&1, ShrinkAxis::Vertical, 32.));
        }
        let output = layout.outputs().next().unwrap().clone();
        layout.interactive_move_begin(1, &output, (100., 10.).into());
        layout.interactive_move_update(&1, (300., 0.).into(), output, (500., 300.).into());
        let request = layout.windows().next().unwrap().1.requested_size();
        layout.request_fullscreen(&1, true);
        assert_eq!(layout.moving_spine_mode.is_some(), spine);
        assert!(layout
            .windows()
            .next()
            .unwrap()
            .1
            .pending_sizing_mode()
            .is_normal());
        assert_eq!(layout.windows().next().unwrap().1.requested_size(), request);
        layout.remove_window(&1, Transaction::new());
        assert!(layout.moving_spine_mode.is_none());
        assert!(layout.interactive_move.is_none());
        assert_eq!(layout.windows().count(), 0);
        layout.verify_invariants();
    }
}

#[test]
fn spine_search_uses_native_slots_and_keeps_scope_separate_from_floating_and_sink() {
    use crate::layout::spine_search::SpineSearchMode;
    for scale in [1., 1.25] {
        let mut options = Options::default();
        options.sink.enabled = true;
        let mut clock = Clock::with_time(Duration::ZERO);
        let mut layout = Layout::with_options(clock.clone(), options);
        Op::AddScaledOutput {
            id: 1,
            scale,
            layout_config: None,
        }
        .apply(&mut layout);
        for id in [1, 2, 3] {
            Op::AddWindow {
                params: TestWindowParams::new(id),
            }
            .apply(&mut layout);
            Op::Communicate(id).apply(&mut layout);
        }
        assert!(layout.commit_shrink(&2, ShrinkAxis::Horizontal, 40.));
        assert!(layout.commit_shrink(&3, ShrinkAxis::Horizontal, 40.));
        let query = |layout: &Layout<TestWindow>, origin| {
            layout
                .active_workspace()
                .unwrap()
                .spine_candidates(origin, |a, b| a.id().cmp(b.id()))
        };
        let requests = layout
            .windows()
            .map(|(_, w)| (*w.id(), w.requested_size()))
            .collect::<Vec<_>>();
        let result = query(&layout, Some(&1)).unwrap();
        assert_eq!(result.mode, SpineSearchMode::Adjacent);
        assert_eq!(result.origin, Some(1));
        assert_eq!(
            result
                .candidates
                .iter()
                .map(|c| (c.id, c.distance))
                .collect::<Vec<_>>(),
            [(2, 1), (3, 2)]
        );
        assert!(query(&layout, None).is_none());
        assert!(query(&layout, Some(&2)).is_none());
        clock.set_unadjusted(Duration::from_millis(100));
        layout.advance_animations();
        assert_eq!(query(&layout, Some(&1)), Some(result));
        assert_eq!(
            layout
                .windows()
                .map(|(_, w)| (*w.id(), w.requested_size()))
                .collect::<Vec<_>>(),
            requests
        );
        layout.set_window_floating(Some(&1), true);
        assert!(query(&layout, Some(&1)).is_none());
        layout.activate_window(&1);
        assert!(layout.sink_window());
        assert_eq!(layout.active_workspace().unwrap().sink_count(), 1);
        assert!(query(&layout, None).is_none());
        assert_eq!(layout.windows().count(), 3);
        layout.verify_invariants();
    }
}

#[test]
fn all_spine_search_includes_hidden_tabs_then_spatial_floats_with_stable_id_ties() {
    use crate::layout::spine_search::SpineSearchMode;
    use niri_ipc::PositionChange;
    let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), Options::default());
    Op::AddOutput(1).apply(&mut layout);
    for id in 1..=4 {
        Op::AddWindow {
            params: TestWindowParams::new(id),
        }
        .apply(&mut layout);
        Op::Communicate(id).apply(&mut layout);
    }
    layout.consume_or_expel_window_left(Some(&2));
    layout.activate_window(&1);
    layout.set_column_display(ColumnDisplay::Tabbed);
    for id in [3, 4] {
        layout.set_window_floating(Some(&id), true);
        Op::Communicate(id).apply(&mut layout);
        layout.move_floating_window(
            Some(&id),
            PositionChange::SetFixed(300.),
            PositionChange::SetFixed(500.),
            false,
        );
        assert!(layout.commit_shrink(&id, ShrinkAxis::Horizontal, 40.));
    }
    assert!(layout.commit_shrink(&1, ShrinkAxis::Horizontal, 40.));
    layout.activate_window(&1);
    let query = |layout: &Layout<TestWindow>| {
        layout
            .active_workspace()
            .unwrap()
            .spine_candidates(None, |a, b| a.id().cmp(b.id()))
    };
    // A hidden Normal tab still prevents a workspace-wide all-spine scope.
    assert!(query(&layout).is_none());
    assert!(layout.commit_shrink(&2, ShrinkAxis::Horizontal, 40.));
    let result = query(&layout).unwrap();
    assert_eq!(result.mode, SpineSearchMode::All);
    assert_eq!(result.origin, None);
    assert_eq!(
        result.candidates.iter().map(|c| c.id).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert!(result.candidates.iter().all(|c| c.distance == 0));
    layout.move_floating_window(
        Some(&4),
        PositionChange::SetFixed(200.),
        PositionChange::SetFixed(500.),
        false,
    );
    assert_eq!(
        query(&layout)
            .unwrap()
            .candidates
            .iter()
            .map(|c| c.id)
            .collect::<Vec<_>>(),
        [1, 2, 4, 3]
    );
    assert!(layout.focus().is_none());
    assert_eq!(layout.windows().count(), 4);
    layout.verify_invariants();
}

#[test]
fn pending_normal_resize_client_choice_keeps_positive_allocation() {
    let mut params = TestWindowParams::new(1);
    params.bbox = Rectangle::from_size((300, 200).into());
    let client = TestWindow::new(params);
    let mut tile = Tile::new(
        client.clone(),
        (1280., 720.).into(),
        1.,
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
    );
    assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
    assert!(
        tile.prepare_normal_restore(crate::layout::shrink_expand::NormalSizePlan {
            allocation: (120., 200.).into(),
            client_size: (120, 0).into(),
            sizing_mode: SizingMode::Normal,
        })
    );
    tile.submit_normal_restore(true, Transaction::new(), false);
    let deadline = client.normal_restore_deadline();
    tile.request_normal_resize_once((140, 0).into(), false);
    assert_eq!(client.requested_size(), Some((140, 0).into()));
    assert_eq!(tile.tile_size(), (140., 200.).into());
    assert_eq!(client.normal_restore_deadline(), deadline);
}

#[test]
fn normal_restore_visual_clock_does_not_control_protocol_or_input_barrier() {
    for deadline_release in [false, true] {
        let mut options = Options::default();
        options.animations.window_resize.anim = niri_config::Animation {
            off: false,
            kind: niri_config::animations::Kind::Easing(niri_config::animations::EasingParams {
                duration_ms: 100,
                curve: niri_config::animations::Curve::Linear,
            }),
        };
        let mut params = TestWindowParams::new(1);
        params.bbox = Rectangle::from_size((300, 200).into());
        let client = TestWindow::new(params);
        let mut clock = Clock::with_time(Duration::ZERO);
        let mut tile = Tile::new(
            client.clone(),
            (1280., 720.).into(),
            1.,
            clock.clone(),
            Rc::new(options),
        );
        assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
        let plan = crate::layout::shrink_expand::NormalSizePlan {
            allocation: tile.normal_tile_size_for_window_size((600, 400).into()),
            client_size: (600, 400).into(),
            sizing_mode: SizingMode::Normal,
        };
        assert!(tile.prepare_normal_restore(plan));
        tile.submit_normal_restore(false, Transaction::new(), false);
        let deadline = client.normal_restore_deadline();
        assert!(tile.are_transitions_ongoing());
        clock.set_unadjusted(Duration::from_millis(200));
        tile.advance_animations();
        assert!(!tile.are_transitions_ongoing());
        assert!(tile.normal_restore_input_blocked());
        assert_eq!(tile.tile_size(), plan.allocation);
        assert_eq!(client.normal_restore_deadline(), deadline);
        if deadline_release {
            client
                .0
                .normal_restore_deadline
                .set(Some(std::time::Instant::now()));
        } else {
            assert!(client.communicate());
        }
        tile.update_window();
        assert!(!tile.normal_restore_input_blocked());
        assert!(tile.are_transitions_ongoing());
        assert_eq!(client.normal_restore_deadline(), None);
        clock.set_unadjusted(Duration::from_millis(400));
        tile.advance_animations();
        assert!(!tile.are_transitions_ongoing());
        // A later native resize must retain its own protocol/animation behavior.
        tile.request_normal_resize_once((650, 450).into(), false);
        assert_eq!(client.requested_size(), Some((650, 450).into()));
        assert!(!tile.normal_restore_input_blocked());
    }
}

#[test]
fn spatial_floating_shrink_commits_the_preview_anchor_and_preserves_native_focus() {
    for animate in [false, true] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            for scale in [1., 1.25] {
                let mut options = Options::default();
                options.shrink_expand.animate = animate;
                let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
                Op::AddScaledOutput {
                    id: 1,
                    scale,
                    layout_config: None,
                }
                .apply(&mut layout);
                for id in [1, 2] {
                    Op::AddWindow {
                        params: TestWindowParams::new(id),
                    }
                    .apply(&mut layout);
                    Op::Communicate(id).apply(&mut layout);
                }
                layout.set_window_floating(Some(&1), true);
                Op::Communicate(1).apply(&mut layout);
                layout.activate_window(&1);
                Op::CompleteAnimations.apply(&mut layout);
                let get = |layout: &Layout<TestWindow>| {
                    let ws = layout
                        .workspaces()
                        .find(|(_, _, ws)| ws.has_window(&1))
                        .unwrap()
                        .2;
                    let (tile, pos, _) = ws
                        .tiles_with_render_positions()
                        .find(|(tile, _, _)| tile.window().id() == &1)
                        .unwrap();
                    (tile.tile_size(), pos, tile.window().requested_size())
                };
                let (source, old_pos, request) = get(&layout);
                let target = if axis == ShrinkAxis::Horizontal {
                    (40., source.h).into()
                } else {
                    (source.w, 40.).into()
                };
                layout.set_shrink_preview(&1, target, false);
                layout.set_shrink_preview_anchor(&1, (1., 1.).into());
                assert!(layout.commit_shrink(&1, axis, 40.));
                let (actual, pos, after_request) = get(&layout);
                assert_eq!(actual, target);
                approx::assert_abs_diff_eq!(pos.x, old_pos.x + source.w - actual.w, epsilon = 1e-6);
                approx::assert_abs_diff_eq!(pos.y, old_pos.y + source.h - actual.h, epsilon = 1e-6);
                assert_eq!(request, after_request);
                assert!(layout.is_spine(&1));
                assert_eq!(layout.focus().map(|focus| *focus.id()), Some(2));
                assert_eq!(layout.windows().count(), 2);
                layout.verify_invariants();
            }
        }
    }
}

#[test]
fn disable_restore_all_workspaces_in_one_batch_keeps_slots_and_focus() {
    for all_books in [false, true] {
        for no_outputs in [false, true] {
            let mut layout =
                Layout::with_options(Clock::with_time(Duration::ZERO), Options::default());
            Op::AddOutput(1).apply(&mut layout);
            for id in 1..=6 {
                Op::AddWindow {
                    params: TestWindowParams::new(id),
                }
                .apply(&mut layout);
                Op::Communicate(id).apply(&mut layout);
            }
            for id in [1, 4] {
                layout.set_window_floating(Some(&id), true);
            }
            layout.consume_or_expel_window_left(Some(&3));
            layout.activate_window(&3);
            layout.set_column_display(ColumnDisplay::Tabbed);
            layout.move_to_workspace(Some(&5), 1, ActivateWindow::No);
            Op::AddOutput(2).apply(&mut layout);
            let output2 = layout
                .outputs()
                .find(|output| output.name() == "output2")
                .unwrap()
                .clone();
            layout.move_to_output(Some(&6), &output2, None, ActivateWindow::No);
            layout.switch_workspace(0);
            layout.activate_window(&3);
            for id in 1..=6 {
                if id == 3 && !all_books {
                    continue;
                }
                let ws = layout
                    .workspaces_mut()
                    .find(|ws| ws.has_window(&id))
                    .unwrap();
                assert!(ws
                    .tiles_mut()
                    .find(|tile| tile.window().id() == &id)
                    .unwrap()
                    .set_spine(
                        if id % 2 == 0 {
                            ShrinkAxis::Vertical
                        } else {
                            ShrinkAxis::Horizontal
                        },
                        40.
                    ));
                ws.update_window(&id, None);
            }
            if no_outputs {
                Op::RemoveOutput(1).apply(&mut layout);
                Op::RemoveOutput(2).apply(&mut layout);
            }
            let focus = layout.focus().map(|w| *w.id());
            let slots = layout
                .workspaces()
                .filter(|(_, _, ws)| ws.windows().next().is_some())
                .map(|(_, _, ws)| {
                    (
                        ws.id(),
                        ws.tiles()
                            .map(|t| (*t.window().id(), ws.floating().has_window(t.window().id())))
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>();
            let identities = layout
                .windows()
                .map(|(_, w)| (*w.id(), w.clone()))
                .collect::<Vec<_>>();
            let batches = layout.restore_all_spines_on_disable(None);
            assert_eq!(batches.len(), 3);
            assert_eq!(
                batches.iter().map(|(_, ids)| ids.len()).sum::<usize>(),
                if all_books { 6 } else { 5 }
            );
            assert_eq!(layout.focus().map(|w| *w.id()), focus);
            assert!(layout
                .workspaces()
                .flat_map(|(_, _, ws)| ws.tiles())
                .all(|t| !t.is_spine()));
            assert_eq!(
                layout
                    .workspaces()
                    .filter(|(_, _, ws)| ws.windows().next().is_some())
                    .map(|(_, _, ws)| (
                        ws.id(),
                        ws.tiles()
                            .map(|t| (*t.window().id(), ws.floating().has_window(t.window().id())))
                            .collect::<Vec<_>>()
                    ))
                    .collect::<Vec<_>>(),
                slots
            );
            for (id, before) in identities {
                let after = layout.windows().find(|(_, w)| w.id() == &id).unwrap().1;
                assert!(Rc::ptr_eq(&before.0, &after.0));
            }
            let deadlines = layout
                .windows()
                .filter_map(|(_, w)| w.normal_restore_deadline())
                .collect::<Vec<_>>();
            assert!(deadlines.iter().all(|d| *d == deadlines[0]));
            assert!(layout.restore_all_spines_on_disable(None).is_empty());
            for id in 1..=6 {
                Op::Communicate(id).apply(&mut layout);
            }
            layout.refresh(true);
            assert_eq!(layout.focus().map(|w| *w.id()), focus);
            layout.verify_invariants();
        }
    }
}

#[test]
fn disable_restore_ends_only_spine_native_move_at_its_final_slot() {
    for spine in [false, true] {
        for floating in [false, true] {
            for detach in [false, true] {
                let mut layout =
                    Layout::with_options(Clock::with_time(Duration::ZERO), Options::default());
                Op::AddOutput(1).apply(&mut layout);
                let mut params = TestWindowParams::new(1);
                params.is_floating = floating;
                Op::AddWindow { params }.apply(&mut layout);
                Op::Communicate(1).apply(&mut layout);
                if spine {
                    let ws = layout
                        .workspaces_mut()
                        .find(|ws| ws.has_window(&1))
                        .unwrap();
                    assert!(ws
                        .tiles_mut()
                        .next()
                        .unwrap()
                        .set_spine(ShrinkAxis::Horizontal, 40.));
                    ws.update_window(&1, None);
                }
                let output = layout.outputs().next().unwrap().clone();
                assert!(layout.interactive_move_begin(1, &output, (25., 100.).into()));
                let delta = if detach { (300., 100.) } else { (20., 0.) };
                assert!(layout.interactive_move_update(
                    &1,
                    delta.into(),
                    output.clone(),
                    (25. + delta.0, 100. + delta.1).into()
                ));
                let focus = layout.focus().map(|w| *w.id());
                let batches = layout.restore_all_spines_on_disable(None);
                assert_eq!(layout.interactive_move.is_none(), spine);
                assert_eq!(batches.is_empty(), !spine);
                assert_eq!(layout.focus().map(|w| *w.id()), focus);
                assert_eq!(layout.windows().count(), 1);
                assert!(!layout.is_spine(&1));
                if !spine {
                    assert!(layout.interactive_move_update(
                        &1,
                        (10., 0.).into(),
                        output,
                        (335., 200.).into()
                    ));
                    layout.interactive_move_end(&1);
                }
                layout.verify_invariants();
            }
        }
    }
}

#[test]
fn spine_resize_normal_insertion_reuses_shared_constraints_and_native_gain() {
    use crate::utils::ResizeEdge;
    for scale in [1., 1.25] {
        for centered in [false, true] {
            for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
                let mut options = Options::default();
                options.layout.border.off = false;
                options.layout.border.width = 2.5;
                options.layout.center_focused_column = if centered {
                    niri_config::CenterFocusedColumn::Always
                } else {
                    niri_config::CenterFocusedColumn::Never
                };
                let options = Rc::new(options);
                let mut space = scrolling(scale, options.clone());
                let mut params = TestWindowParams::new(1);
                params.min_max_size = ((500, 100).into(), (0, 0).into());
                let mut ordinary = Tile::new(
                    TestWindow::new(params),
                    (1280., 720.).into(),
                    scale,
                    Clock::with_time(Duration::ZERO),
                    options.clone(),
                );
                ordinary.clock = space.clock().clone();
                space.add_tile(None, ordinary, true, ColumnWidth::Fixed(800.), false, None);
                let mut params = TestWindowParams::new(2);
                params.min_max_size = ((150, 120).into(), (900, 500).into());
                let client = TestWindow::new(params);
                let mut book = Tile::new(
                    client.clone(),
                    (1280., 720.).into(),
                    scale,
                    Clock::with_time(Duration::ZERO),
                    options,
                );
                book.clock = space.clock().clone();
                assert!(book.set_spine(axis, 40.1));
                space.add_tile_to_column(0, None, book, false);
                let edges = match axis {
                    ShrinkAxis::Horizontal => ResizeEdge::LEFT,
                    ShrinkAxis::Vertical => ResizeEdge::TOP,
                };
                assert!(space.interactive_resize_begin(2, edges));
                let delta = (-180., -180.).into();
                let intent = space.spatial_resize_intent(&2, delta).unwrap();
                assert!(intent.grows_locked_axis(axis));
                let requests = [(2, ColumnWidth::Fixed(760.))];
                let plans = space.normal_spine_plans_with_resize(&requests, Some((&2, intent)));
                let plan = plans[0].1;
                assert!(
                    plan.client_size.w >= 500,
                    "shared ordinary minimum must survive"
                );
                assert!(plan.client_size.h >= 120 && plan.client_size.h <= 500);
                assert!(space
                    .tiles_mut()
                    .find(|t| t.window().id() == &2)
                    .unwrap()
                    .prepare_normal_restore(plan));
                space.commit_normal_restore_with_resize(
                    &plans,
                    &requests,
                    Transaction::new(),
                    true,
                    Some((&2, intent)),
                );
                space.rebase_spine_resize(&2, delta, plan);
                assert_eq!(client.requested_size(), Some(plan.client_size));
                space.interactive_resize_update(&2, delta);
                assert_eq!(
                    client.requested_size(),
                    Some(plan.client_size),
                    "same pointer position must not jump"
                );
                let raw = space
                    .spatial_resize_intent(&2, (-200., -200.).into())
                    .unwrap();
                match axis {
                    ShrinkAxis::Horizontal => {
                        assert!(
                            (raw.requested.w - plan.allocation.w + 20. * intent.gain.x).abs()
                                < 1e-6
                        )
                    }
                    ShrinkAxis::Vertical => {
                        assert!(
                            (raw.requested.h - plan.allocation.h + 20. * intent.gain.y).abs()
                                < 1e-6
                        )
                    }
                }
                settle(&mut space);
                space.verify_invariants();
                assert_eq!(space.columns().count(), 1);
                assert_eq!(
                    space.tiles().map(|t| *t.window().id()).collect::<Vec<_>>(),
                    vec![1, 2]
                );
            }
        }
    }
}

#[test]
fn spine_resize_floating_normal_constraints_keep_opposite_edges_without_double_shift() {
    use crate::utils::ResizeEdge;
    for scale in [1., 1.25] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            let mut options = Options::default();
            options.layout.border.off = false;
            options.layout.border.width = 2.5;
            let options = Rc::new(options);
            let mut params = TestWindowParams::new(1);
            params.min_max_size = ((150, 120).into(), (400, 300).into());
            let client = TestWindow::new(params);
            let mut book = Tile::new(
                client.clone(),
                (1280., 720.).into(),
                scale,
                Clock::with_time(Duration::ZERO),
                options.clone(),
            );
            assert!(book.set_spine(axis, 40.1));
            let mut space = FloatingSpace::new(
                (1280., 720.).into(),
                Rectangle::from_size((1280., 720.).into()),
                scale,
                Clock::with_time(Duration::ZERO),
                options,
            );
            book.clock = space.clock().clone();
            space.add_tile(book, false);
            assert!(space.interactive_resize_begin(1, ResizeEdge::LEFT | ResizeEdge::TOP));
            let (old_size, old_pos) = space
                .tiles_with_offsets()
                .map(|(t, p)| (t.tile_size(), p))
                .next()
                .unwrap();
            let delta = (-700., -700.).into();
            let intent = space.spatial_resize_intent(&1, delta).unwrap();
            let plans = space.normal_spine_plans_with_resize(&[1], Some((&1, intent)));
            let plan = plans[0].1;
            assert_eq!(plan.client_size, (400, 300).into());
            space.anchor_spine_restore(&1, plan);
            assert!(space
                .tiles_mut()
                .next()
                .unwrap()
                .prepare_normal_restore(plan));
            space.commit_normal_restore(&plans, Transaction::new(), true);
            space.rebase_spine_resize(&1, delta, plan);
            let (_, new_pos) = space.tiles_with_offsets().next().unwrap();
            assert!((old_pos.x + old_size.w - new_pos.x - plan.allocation.w).abs() < 1e-6);
            assert!((old_pos.y + old_size.h - new_pos.y - plan.allocation.h).abs() < 1e-6);
            space.interactive_resize_update(&1, delta);
            assert_eq!(space.tiles_with_offsets().next().unwrap().1, new_pos);
            assert_eq!(client.requested_size(), Some(plan.client_size));
            for _ in 0..4 {
                if client.communicate() {
                    space.update_window(&1, None);
                }
            }
            assert_eq!(space.tiles_with_offsets().next().unwrap().1, new_pos);
            space.verify_invariants();
        }
    }
}

#[test]
fn spatial_raw_query_preserves_native_edges_centering_and_topmost_restriction() {
    for centered in [false, true] {
        let mut options = Options::default();
        options.layout.center_focused_column = if centered {
            niri_config::CenterFocusedColumn::Always
        } else {
            niri_config::CenterFocusedColumn::Never
        };
        let options = Rc::new(options);
        let mut space = scrolling(1.25, options.clone());
        let tile = scrolling_tile(1, &space, options);
        space.add_tile(None, tile, true, ColumnWidth::Fixed(900.), false, None);
        settle(&mut space);
        assert!(space.interactive_resize_begin(
            1,
            crate::utils::ResizeEdge::LEFT | crate::utils::ResizeEdge::TOP
        ));
        let before = space.tiles().next().unwrap().window().requested_size();
        let raw = space.spatial_resize_intent(&1, (10., 100.).into()).unwrap();
        assert_eq!(raw.gain.x, if centered { -2. } else { -1. });
        assert_eq!(raw.gain.y, 0.);
        assert_eq!(raw.requested.h, raw.original.h);
        assert_eq!(
            raw.original.w - raw.requested.w,
            if centered { 20. } else { 10. }
        );
        assert_eq!(
            space.tiles().next().unwrap().window().requested_size(),
            before
        );
        space.interactive_resize_update(&1, (10., 100.).into());
        assert_eq!(
            space
                .tiles()
                .next()
                .unwrap()
                .window()
                .requested_size()
                .unwrap()
                .w as f64,
            raw.requested.w.round()
        );
        space.verify_invariants();
    }
}

#[test]
fn spatial_zone_zero_is_strict_and_corner_selection_has_stable_ties() {
    use crate::layout::shrink_expand::SpatialResizeIntent;
    let mut raw = SpatialResizeIntent {
        original: (300., 300.).into(),
        requested: (100., 100.).into(),
        minimum: (100., 100.).into(),
        gain: (1., 1.).into(),
        delta_origin: (0., 0.).into(),
    };
    assert_eq!(raw.choose_axis(0., [true, true], None), None);
    assert_eq!(
        raw.choose_axis(24., [true, true], None),
        Some(ShrinkAxis::Horizontal)
    );
    assert_eq!(
        raw.choose_axis(24., [true, true], Some(ShrinkAxis::Vertical)),
        Some(ShrinkAxis::Vertical)
    );
    raw.requested.w = 99.;
    assert_eq!(
        raw.choose_axis(0., [true, true], None),
        Some(ShrinkAxis::Horizontal)
    );
    raw.gain.x = 0.;
    assert_eq!(raw.choose_axis(0., [true, true], None), None);
    raw.requested.h = 90.;
    assert_eq!(
        raw.choose_axis(0., [true, true], None),
        Some(ShrinkAxis::Vertical)
    );
    raw.gain = (1., 1.).into();
    raw.requested = (-500., -600.).into();
    assert_eq!(
        raw.choose_axis(24., [true, true], None),
        Some(ShrinkAxis::Vertical)
    );
}

#[test]
fn keyboard_boundary_query_uses_shared_column_tab_constraints_without_mutation() {
    use std::cmp::Ordering;
    for scale in [1., 1.25] {
        for display in [ColumnDisplay::Normal, ColumnDisplay::Tabbed] {
            let mut options = Options::default();
            options.layout.default_column_display = display;
            options.layout.border.off = false;
            options.layout.border.width = 1.3;
            let options = Rc::new(options);
            let mut space = scrolling(scale, options.clone());
            let mut clients = Vec::new();
            for id in 1..=2 {
                let mut params = TestWindowParams::new(id);
                params.bbox = Rectangle::from_size((700, 500).into());
                params.min_max_size = (
                    (
                        if id == 1 { 100 } else { 240 },
                        if id == 1 { 100 } else { 180 },
                    )
                        .into(),
                    (0, 0).into(),
                );
                let client = TestWindow::new(params);
                let tile = Tile::new(
                    client.clone(),
                    space.view_size(),
                    scale,
                    space.clock().clone(),
                    options.clone(),
                );
                if id == 1 {
                    space.add_tile(None, tile, true, ColumnWidth::Fixed(700.), false, None);
                } else {
                    space.add_tile_to_column(0, None, tile, false);
                }
                clients.push(client);
            }
            settle(&mut space);
            assert!(
                !space
                    .keyboard_resize_intent(
                        &1,
                        ShrinkAxis::Horizontal,
                        SizeChange::AdjustFixed(-10)
                    )
                    .unwrap()
                    .at_minimum
            );
            space.set_window_width(Some(&1), SizeChange::SetFixed(0));
            space.set_window_height(Some(&1), SizeChange::SetFixed(0));
            settle(&mut space);
            let requests = clients
                .iter()
                .map(TestWindow::requested_size)
                .collect::<Vec<_>>();
            let sizes = space
                .tiles()
                .map(|tile| tile.tile_size())
                .collect::<Vec<_>>();
            for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
                let intent = space
                    .keyboard_resize_intent(&1, axis, SizeChange::AdjustProportion(-0.1))
                    .unwrap();
                assert!(intent.at_minimum, "{scale} {display:?} {axis:?}");
                assert_eq!(intent.direction, Ordering::Less);
                assert_eq!(
                    space
                        .keyboard_resize_intent(&1, axis, SizeChange::AdjustFixed(0))
                        .unwrap()
                        .direction,
                    Ordering::Equal
                );
                assert_eq!(
                    space
                        .keyboard_resize_intent(&1, axis, SizeChange::SetFixed(700))
                        .unwrap()
                        .direction,
                    Ordering::Greater
                );
            }
            assert_eq!(
                requests,
                clients
                    .iter()
                    .map(TestWindow::requested_size)
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                sizes,
                space
                    .tiles()
                    .map(|tile| tile.tile_size())
                    .collect::<Vec<_>>()
            );
            assert!(clients[0].size().w >= 240);
            if display == ColumnDisplay::Tabbed {
                assert!(clients[0].size().h >= 180);
            }
            space.verify_invariants();
        }
    }
}

fn tile(id: usize, scale: f64, options: Rc<Options>) -> Tile<TestWindow> {
    let mut params = TestWindowParams::new(id);
    params.bbox = Rectangle::from_size((900, 700).into());
    params.min_max_size = ((600, 500).into(), (1200, 1000).into());
    Tile::new(
        TestWindow::new(params),
        (1280., 720.).into(),
        scale,
        Clock::with_time(Duration::ZERO),
        options,
    )
}

fn scrolling(scale: f64, options: Rc<Options>) -> ScrollingSpace<TestWindow> {
    ScrollingSpace::new(
        (1280., 720.).into(),
        Rectangle::from_size((1280., 720.).into()),
        scale,
        Clock::with_time(Duration::ZERO),
        options,
    )
}

fn scrolling_tile(
    id: usize,
    space: &ScrollingSpace<TestWindow>,
    options: Rc<Options>,
) -> Tile<TestWindow> {
    let mut tile = tile(id, 1., options);
    tile.clock = space.clock().clone();
    tile
}

fn settle(space: &mut ScrollingSpace<TestWindow>) {
    // Emulate asynchronous normal clients; spines need no commit to assume their allocation.
    for _ in 0..4 {
        let changed = space
            .tiles()
            .filter_map(|tile| tile.window().communicate().then_some(*tile.window().id()))
            .collect::<Vec<_>>();
        for id in changed {
            space.update_window(&id, None);
        }
    }
    space.verify_invariants();
}

#[test]
fn current_slot_normal_plan_uses_live_defaults_rules_and_native_insertion_width() {
    for rule_width in [None, Some(Some(PresetSize::Fixed(850))), Some(None)] {
        let mut options = Options::default();
        options.layout.default_column_width = Some(PresetSize::Fixed(720));
        let mut workspace =
            Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::new(options.clone()));
        let mut params = TestWindowParams::new(1);
        params.bbox = Rectangle::from_size((900, 300).into());
        params.rules = Some(crate::window::ResolvedWindowRules {
            default_width: rule_width,
            ..Default::default()
        });
        let client = TestWindow::new(params);
        let mut book = workspace.make_tile(client.clone());
        assert!(book.set_spine(ShrinkAxis::Horizontal, 40.));
        workspace.add_tile(
            book,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            ColumnWidth::Fixed(1100.),
            true,
            false,
            None,
        );
        // Both the current configuration and the client's own size can change while folded.
        options.layout.default_column_width = Some(PresetSize::Fixed(760));
        workspace.update_config(Rc::new(options.clone()));
        client.0.bbox.set(Rectangle::from_size((680, 300).into()));
        workspace.update_window(&1, None);
        let normal = workspace.make_tile(TestWindow::new(TestWindowParams::new(2)));
        workspace.add_tile(
            normal,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            ColumnWidth::Fixed(400.),
            false,
            false,
            None,
        );
        let sizes = workspace.tiles().map(|t| t.tile_size()).collect::<Vec<_>>();
        let focus = workspace.selected_window().map(|w| *w.id());
        let plans = workspace.normal_spine_plans(&[99, 1, 2, 1]);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].0, 1);
        assert_eq!(
            workspace.tiles().map(|t| t.tile_size()).collect::<Vec<_>>(),
            sizes
        );
        assert_eq!(workspace.selected_window().map(|w| *w.id()), focus);
        assert_eq!(client.requested_size(), None);
        assert!(client.0.output_events.borrow().is_empty());
        let width = match rule_width {
            None => 760.,
            Some(Some(_)) => 850.,
            Some(None) => 680.,
        };
        let mut oracle =
            Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::new(options));
        let ordinary = TestWindow::new(TestWindowParams::new(1));
        let tile = oracle.make_tile(ordinary.clone());
        oracle.add_tile(
            tile,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            ColumnWidth::Fixed(width),
            false,
            false,
            None,
        );
        assert_eq!(plans[0].1.client_size, ordinary.requested_size().unwrap());
        assert_eq!(plans[0].1.sizing_mode, SizingMode::Normal);
        assert_ne!(plans[0].1.client_size.w, 1100);
        assert_eq!(workspace.windows().count(), 2);
        assert!(workspace
            .tiles()
            .find(|t| t.window().id() == &1)
            .unwrap()
            .is_spine());
    }
}

#[test]
fn batch_normal_plans_replace_all_selected_constraints_in_one_column_snapshot() {
    for scale in [1., 1.25] {
        for display in [ColumnDisplay::Normal, ColumnDisplay::Tabbed] {
            for fixed_height in [false, true] {
                let mut options = Options::default();
                options.layout.default_column_display = display;
                options.layout.border.off = false;
                options.layout.border.width = 1.3;
                let options = Rc::new(options);
                let mut space = scrolling(scale, options.clone());
                let mut clients = Vec::new();
                for id in 1..=3 {
                    let mut params = TestWindowParams::new(id);
                    params.min_max_size = ((100, 20).into(), (0, 0).into());
                    let client = TestWindow::new(params);
                    let mut tile = Tile::new(
                        client.clone(),
                        space.view_size(),
                        scale,
                        space.clock().clone(),
                        options.clone(),
                    );
                    if id > 1 {
                        assert!(tile.set_spine(ShrinkAxis::Vertical, 40.));
                    }
                    if id == 1 {
                        space.add_tile(None, tile, true, ColumnWidth::Fixed(700.), false, None);
                    } else {
                        space.add_tile_to_column(0, None, tile, false);
                    }
                    clients.push(client);
                }
                settle(&mut space);
                if fixed_height {
                    space.set_window_height(Some(&2), SizeChange::SetFixed(120));
                }
                let requests_before = clients
                    .iter()
                    .map(TestWindow::requested_size)
                    .collect::<Vec<_>>();
                let sizes_before = space.tiles().map(|t| t.tile_size()).collect::<Vec<_>>();
                let plans = space.normal_spine_plans(&[
                    (3, ColumnWidth::Fixed(1200.)),
                    (1, ColumnWidth::Fixed(1200.)),
                    (2, ColumnWidth::Fixed(1200.)),
                    (2, ColumnWidth::Fixed(100.)),
                ]);
                assert_eq!(plans.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [2, 3]);
                assert_eq!(
                    clients
                        .iter()
                        .map(TestWindow::requested_size)
                        .collect::<Vec<_>>(),
                    requests_before
                );
                assert_eq!(
                    space.tiles().map(|t| t.tile_size()).collect::<Vec<_>>(),
                    sizes_before
                );
                let single = space.normal_spine_plans(&[(2, ColumnWidth::Fixed(1200.))]);
                if display == ColumnDisplay::Normal && !fixed_height {
                    assert_ne!(single[0].1.allocation.h, plans[0].1.allocation.h);
                }
                // The ordinary allocator is the oracle, with the same members and native Auto(1)
                // insertion height. Clearing form here is a test setup, not a user Expand action.
                for tile in space.tiles_mut().filter(|t| t.is_spine()) {
                    tile.clear_spine();
                }
                space.reset_window_height(Some(&2));
                space.reset_window_height(Some(&3));
                settle(&mut space);
                for (id, plan) in plans {
                    assert_eq!(plan.client_size, clients[id - 1].requested_size().unwrap());
                    assert_eq!(plan.sizing_mode, SizingMode::Normal);
                }
                assert_eq!(space.columns().count(), 1);
                assert_eq!(
                    space.tiles().map(|t| *t.window().id()).collect::<Vec<_>>(),
                    [1, 2, 3]
                );
            }
        }
    }
}

#[test]
fn initial_spine_geometry_rejects_pending_and_committed_windowed_fullscreen() {
    let mut tile = tile(1, 1., Rc::default());
    let client = tile.window().clone();
    client.0.is_pending_windowed_fullscreen.set(true);
    assert!(tile
        .plan_spine_geometry(ShrinkAxis::Horizontal, 40.)
        .is_none());
    assert!(!tile.set_spine(ShrinkAxis::Horizontal, 40.));
    assert_eq!(tile.form(), WindowForm::Normal);
    client.communicate();
    tile.update_window();
    client.0.is_pending_windowed_fullscreen.set(false);
    assert!(tile
        .plan_spine_geometry(ShrinkAxis::Vertical, 40.)
        .is_none());
    client.communicate();
    tile.update_window();
    assert!(tile
        .plan_spine_geometry(ShrinkAxis::Vertical, 40.)
        .is_some());
}

#[test]
fn normal_slot_plans_in_non_normal_tabs_inherit_native_container_request() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let mut space = non_normal_space(1.25, mode, ColumnDisplay::Tabbed);
        let options = space.tiles().next().unwrap().options().clone();
        let mut book = scrolling_tile(2, &space, options);
        let client = book.window().clone();
        assert!(book.set_spine(ShrinkAxis::Vertical, 40.));
        space.add_tile_to_column(0, None, book, true);
        let before = space.tiles().map(|t| t.tile_size()).collect::<Vec<_>>();
        let plans = space.normal_spine_plans(&[(2, ColumnWidth::Fixed(800.))]);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].1.sizing_mode, mode);
        assert_eq!(
            plans[0].1.client_size,
            space
                .tiles()
                .next()
                .unwrap()
                .window()
                .requested_size()
                .unwrap()
        );
        assert_eq!(
            plans[0].1.allocation,
            if mode.is_fullscreen() {
                space.view_size()
            } else {
                space.parent_area().size
            }
        );
        assert_eq!(client.requested_size(), None);
        assert_eq!(client.pending_sizing_mode(), SizingMode::Normal);
        assert_eq!(
            space.tiles().map(|t| t.tile_size()).collect::<Vec<_>>(),
            before
        );
        settle(&mut space);
    }
}

#[test]
fn floating_normal_plans_use_native_defaults_without_size_memory_or_recentering() {
    for explicit_size in [false, true] {
        let mut options = Options::default();
        options.layout.default_column_width = Some(PresetSize::Fixed(1000));
        let options = Rc::new(options);
        let mut params = TestWindowParams::new(1);
        params.bbox = Rectangle::from_size((240, 160).into());
        if explicit_size {
            params.min_max_size = ((600, 500).into(), (800, 600).into());
            params.rules = Some(crate::window::ResolvedWindowRules {
                default_width: Some(Some(PresetSize::Fixed(720))),
                default_height: Some(Some(PresetSize::Fixed(400))),
                ..Default::default()
            });
        }
        let client = TestWindow::new(params);
        let clock = Clock::with_time(Duration::ZERO);
        let mut book = Tile::new(
            client.clone(),
            (1280., 720.).into(),
            1.25,
            clock.clone(),
            options.clone(),
        );
        book.floating_window_size = Some((999, 888).into());
        book.floating_pos = Some((0.25, 0.75).into());
        assert!(book.set_spine(ShrinkAxis::Horizontal, 40.));
        let mut space = FloatingSpace::new(
            (1280., 720.).into(),
            Rectangle::from_size((1280., 720.).into()),
            1.25,
            clock,
            options,
        );
        space.add_tile(book, true);
        let before = space
            .tiles_with_offsets()
            .map(|(tile, pos)| (tile.tile_size(), pos))
            .collect::<Vec<_>>();
        let plan = space.normal_spine_plans(&[99, 1, 1]);
        assert_eq!(plan.len(), 1);
        assert_eq!(
            plan[0].1.client_size,
            if explicit_size {
                (720, 500).into()
            } else {
                (0, 0).into()
            }
        );
        assert_eq!(
            plan[0].1.allocation,
            if explicit_size {
                (720., 500.).into()
            } else {
                (240., 160.).into()
            }
        );
        assert_eq!(plan[0].1.sizing_mode, SizingMode::Normal);
        assert_eq!(client.requested_size(), None);
        assert_eq!(
            space.tiles().next().unwrap().floating_window_size,
            Some((999, 888).into())
        );
        assert_eq!(
            space
                .tiles_with_offsets()
                .map(|(tile, pos)| (tile.tile_size(), pos))
                .collect::<Vec<_>>(),
            before
        );
        space.verify_invariants();
    }
}

fn non_normal_space(
    scale: f64,
    mode: SizingMode,
    display: ColumnDisplay,
) -> ScrollingSpace<TestWindow> {
    let mut options = Options::default();
    options.layout.default_column_display = display;
    options.layout.struts.top = niri_config::FloatOrInt(13.);
    options.layout.struts.bottom = niri_config::FloatOrInt(11.);
    let options = Rc::new(options);
    let mut space = ScrollingSpace::new(
        (1280., 720.).into(),
        Rectangle::new((0., 24.).into(), (1280., 664.).into()),
        scale,
        Clock::with_time(Duration::ZERO),
        options.clone(),
    );
    let first = scrolling_tile(1, &space, options);
    space.add_tile(None, first, true, ColumnWidth::Fixed(800.), false, None);
    match mode {
        SizingMode::Fullscreen => assert!(space.set_fullscreen(&1, true)),
        SizingMode::Maximized => assert!(space.set_maximized(&1, true)),
        SizingMode::Normal => unreachable!(),
    }
    settle(&mut space);
    space
}

#[test]
fn passive_spine_insertion_into_fullscreen_or_maximized_tabs_keeps_client_normal() {
    for scale in [1., 1.25] {
        for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
            for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
                for activate in [false, true] {
                    let mut space = non_normal_space(scale, mode, ColumnDisplay::Tabbed);
                    let normal = space.tiles().next().unwrap().window().clone();
                    let normal_request = normal.requested_size();
                    let options = space.tiles().next().unwrap().options().clone();
                    let mut incoming = scrolling_tile(2, &space, options);
                    let client = incoming.window().clone();
                    assert!(incoming.set_spine(axis, 40.1));
                    let client_size = client.size();
                    let plans = space.spine_in_column_plans(0, 0, &incoming, 40.1).unwrap();
                    assert_eq!(client.requested_size(), None);
                    assert_eq!(normal.requested_size(), normal_request);
                    assert!(plans.get(axis).fits);
                    assert_eq!(plans.for_move(axis).geometry.axis, axis);
                    let extent = if mode.is_fullscreen() { 720. } else { 664. };
                    let thickness = (40.1_f64 * scale).ceil() / scale;
                    let expected = match axis {
                        ShrinkAxis::Horizontal => (thickness, extent),
                        ShrinkAxis::Vertical => (1280., thickness),
                    };
                    assert_eq!(plans.get(axis).geometry.outer_size(), expected.into());
                    space.add_tile_to_column(0, Some(0), incoming, activate);
                    settle(&mut space);
                    assert_eq!(space.columns().count(), 1);
                    assert_eq!(
                        space.tiles().map(|t| *t.window().id()).collect::<Vec<_>>(),
                        [2, 1]
                    );
                    let col = space.columns().next().unwrap();
                    assert_eq!(col.pending_sizing_mode(), mode);
                    assert_eq!(
                        space.active_window().unwrap().id(),
                        if activate { &2 } else { &1 }
                    );
                    let book = space.tiles().find(|t| t.window().id() == &2).unwrap();
                    assert_eq!(book.form(), WindowForm::Spine(axis));
                    assert_eq!(book.tile_size(), expected.into());
                    assert_eq!(book.tile_size(), plans.get(axis).geometry.outer_size());
                    assert_eq!(book.sizing_mode(), SizingMode::Normal);
                    assert_eq!(client.pending_sizing_mode(), SizingMode::Normal);
                    assert_eq!(client.sizing_mode(), SizingMode::Normal);
                    assert_eq!(client.requested_size(), None);
                    assert_eq!(client.size(), client_size);
                    assert_eq!(normal.pending_sizing_mode(), mode);
                    assert_eq!(normal.requested_size(), normal_request);
                    assert!(space.spine_plans(&1, 40.1).is_none());
                    assert_eq!(
                        space
                            .spine_plans(&2, 40.1)
                            .unwrap()
                            .get(axis)
                            .geometry
                            .outer_size(),
                        book.tile_size()
                    );
                }
            }
        }
    }
}

#[test]
fn closing_last_normal_tab_keeps_spine_container_origin_until_native_mode_changes() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            let mut space = non_normal_space(1., mode, ColumnDisplay::Tabbed);
            let options = space.tiles().next().unwrap().options().clone();
            let mut incoming = scrolling_tile(2, &space, options.clone());
            let client = incoming.window().clone();
            assert!(incoming.set_spine(axis, 40.));
            space.add_tile_to_column(0, Some(1), incoming, true);
            settle(&mut space);
            let before = space
                .tiles()
                .find(|t| t.window().id() == &2)
                .unwrap()
                .tile_size();
            let removed = space.remove_tile(&1, Transaction::new());
            assert_eq!(removed.tile.window().id(), &1);
            settle(&mut space);
            assert_eq!(space.columns().count(), 1);
            assert_eq!(space.tiles().count(), 1);
            let col = space.columns().next().unwrap();
            let (book, pos) = col.tiles().next().unwrap();
            assert_eq!(book.tile_size(), before);
            assert_eq!(pos.y, if mode.is_fullscreen() { 0. } else { 24. });
            assert_eq!(col.pending_sizing_mode(), mode);
            // Output/config changes allocate the spine immediately, without a hidden commit.
            space.update_config(
                (1000., 600.).into(),
                Rectangle::new((0., 20.).into(), (1000., 550.).into()),
                1.25,
                options,
            );
            settle(&mut space);
            let expected = match axis {
                ShrinkAxis::Horizontal => (40., if mode.is_fullscreen() { 600. } else { 550.4 }),
                ShrinkAxis::Vertical => (1000., 40.),
            };
            assert_eq!(space.tiles().next().unwrap().tile_size(), expected.into());
            client.0.bbox.set(Rectangle::from_size((700, 500).into()));
            space.update_window(&2, None);
            assert_eq!(space.tiles().next().unwrap().tile_size(), expected.into());
            assert_eq!(client.pending_sizing_mode(), SizingMode::Normal);
            assert_eq!(client.requested_size(), None);
            // This internal mode removal must keep the spine folded (MoveGrab uses it too).
            if mode.is_fullscreen() {
                assert!(space.set_fullscreen(&2, false));
            } else {
                assert!(space.set_maximized(&2, false));
            }
            settle(&mut space);
            assert_eq!(
                space.tiles().next().unwrap().form(),
                WindowForm::Spine(axis)
            );
            assert_eq!(
                space.columns().next().unwrap().pending_sizing_mode(),
                SizingMode::Normal
            );
            assert_eq!(client.requested_size(), None);
        }
    }
}

#[test]
fn spine_insertion_into_non_tabbed_fullscreen_column_uses_native_mode_removal() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            let mut space = non_normal_space(1., mode, ColumnDisplay::Normal);
            let normal = space.tiles().next().unwrap().window().clone();
            let options = space.tiles().next().unwrap().options().clone();
            let mut incoming = scrolling_tile(2, &space, options);
            let client = incoming.window().clone();
            assert!(incoming.set_spine(axis, 40.));
            let plans = space.spine_in_column_plans(0, 1, &incoming, 40.).unwrap();
            space.add_tile_to_column(0, Some(1), incoming, true);
            assert_eq!(
                space
                    .tiles()
                    .find(|t| t.window().id() == &2)
                    .unwrap()
                    .tile_size(),
                plans.get(axis).geometry.outer_size()
            );
            settle(&mut space);
            assert_eq!(space.columns().count(), 1);
            assert_eq!(space.tiles().count(), 2);
            assert_eq!(
                space.columns().next().unwrap().pending_sizing_mode(),
                SizingMode::Normal
            );
            assert_eq!(normal.pending_sizing_mode(), SizingMode::Normal);
            assert_eq!(client.pending_sizing_mode(), SizingMode::Normal);
            assert_eq!(client.requested_size(), None);
            assert_eq!(
                space
                    .tiles()
                    .find(|t| t.window().id() == &2)
                    .unwrap()
                    .form(),
                WindowForm::Spine(axis)
            );
        }
    }
}

#[test]
fn spine_only_non_normal_tabs_accept_normal_clients_before_their_first_commit() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            let mut space = non_normal_space(1., mode, ColumnDisplay::Tabbed);
            let options = space.tiles().next().unwrap().options().clone();
            let mut book = scrolling_tile(2, &space, options.clone());
            let hidden = book.window().clone();
            assert!(book.set_spine(axis, 40.));
            space.add_tile_to_column(0, None, book, true);
            space.remove_tile(&1, Transaction::new());
            space.verify_invariants();
            let size = space.tiles().next().unwrap().tile_size();
            let incoming = scrolling_tile(3, &space, options);
            let client = incoming.window().clone();
            space.add_tile_to_column(0, Some(0), incoming, true);
            assert_eq!(client.sizing_mode(), SizingMode::Normal);
            assert_eq!(client.pending_sizing_mode(), mode);
            space.verify_invariants();
            assert_eq!(
                space
                    .tiles()
                    .find(|t| t.window().id() == &2)
                    .unwrap()
                    .tile_size(),
                size
            );
            settle(&mut space);
            assert_eq!(client.sizing_mode(), mode);
            assert_eq!(hidden.pending_sizing_mode(), SizingMode::Normal);
            assert_eq!(hidden.requested_size(), None);
            assert_eq!(space.columns().count(), 1);
            assert_eq!(
                space.tiles().map(|t| *t.window().id()).collect::<Vec<_>>(),
                [3, 2]
            );
        }
    }
}

#[test]
fn spine_geometry_is_independent_of_client_and_locks_only_one_axis() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let mut tile = tile(1, 1., Rc::default());
        let client = tile.window().clone();
        let client_size = client.size();
        let requested = client.requested_size();
        assert!(tile.set_spine(axis, 40.));
        tile.request_tile_size((300., 200.).into(), false, Some(Transaction::new()));
        let expected = match axis {
            ShrinkAxis::Horizontal => (40., 200.),
            ShrinkAxis::Vertical => (300., 40.),
        };
        assert_eq!(tile.form(), WindowForm::Spine(axis));
        assert_eq!(tile.tile_size(), Size::from(expected));
        assert_eq!(tile.animated_tile_size(), tile.tile_size());
        assert_eq!(tile.tile_expected_or_current_size(), tile.tile_size());
        assert_eq!(client.size(), client_size);
        assert_eq!(client.requested_size(), requested);
        assert!(client.0.output_events.borrow().is_empty());
        assert!(Rc::ptr_eq(&client.0, &tile.window().0));
        tile.request_tile_size((1., 1.).into(), false, None);
        assert_eq!(tile.tile_size(), Size::from((40., 40.)));
        assert!(tile.clear_spine());
        assert_eq!(tile.form(), WindowForm::Normal);
        assert!(!tile.clear_spine());
        // Clearing form alone neither restores old layout nor sends a client configure.
        assert_eq!(client.requested_size(), requested);
    }
}

#[test]
fn thickness_includes_border_and_rounds_once_to_physical_pixels() {
    for scale in [1., 1.25, 1.5, 2.] {
        let mut geometry =
            SpineGeometry::new(ShrinkAxis::Horizontal, 10.1, (1., 100.3).into(), scale, 8.)
                .unwrap();
        let t = geometry.effective_thickness(scale, 8.);
        assert!(t >= 16. + 1. / scale);
        assert_eq!(t * scale, (t * scale).round());
        assert_eq!(geometry.outer_size().w, t);
        geometry.allocate((10000., 1.).into(), scale, 8.);
        assert_eq!(geometry.outer_size(), Size::from((t, t)));
    }
    for invalid in [0., -1., f64::NAN, f64::INFINITY] {
        assert!(
            SpineGeometry::new(ShrinkAxis::Horizontal, invalid, (100., 100.).into(), 1., 0.)
                .is_none()
        );
    }
}

#[test]
fn shared_column_keeps_horizontal_spine_lock_and_native_normal_width() {
    for tabbed in [false, true] {
        let mut options = Options::default();
        options.layout.border.off = true;
        options.layout.tab_indicator.off = true;
        options.layout.default_column_display = if tabbed {
            ColumnDisplay::Tabbed
        } else {
            ColumnDisplay::Normal
        };
        let options = Rc::new(options);
        let mut space = scrolling(1., options.clone());
        let mut spine = scrolling_tile(1, &space, options.clone());
        let client = spine.window().clone();
        assert!(spine.set_spine(ShrinkAxis::Horizontal, 40.));
        space.add_tile(None, spine, true, ColumnWidth::Fixed(400.), false, None);
        settle(&mut space);
        assert_eq!(space.tiles().next().unwrap().tile_size().w, 40.);
        assert_eq!(client.requested_size(), None);

        // This is the actual native InColumn insertion, without a replacement NewColumn.
        let normal = scrolling_tile(2, &space, options);
        let normal_client = normal.window().clone();
        space.add_tile_to_column(0, Some(1), normal, true);
        settle(&mut space);
        assert_eq!(space.columns().count(), 1);
        assert_eq!(space.columns().next().unwrap().tiles().count(), 2);
        let spine = space.tiles().find(|tile| tile.window().id() == &1).unwrap();
        assert_eq!(spine.form(), WindowForm::Spine(ShrinkAxis::Horizontal));
        assert_eq!(spine.tile_size().w, 40.);
        assert!(spine.tile_size().h >= 40.);
        assert_eq!(client.requested_size(), None);
        assert_eq!(normal_client.requested_size().unwrap().w, 600);
        assert_eq!(normal_client.size().w, 600);
    }
}

#[test]
fn vertical_spines_release_height_to_normal_tiles() {
    let mut options = Options::default();
    options.layout.border.off = true;
    let options = Rc::new(options);
    let mut space = scrolling(1., options.clone());
    let mut spine = scrolling_tile(1, &space, options.clone());
    assert!(spine.set_spine(ShrinkAxis::Vertical, 40.));
    space.add_tile(None, spine, true, ColumnWidth::Fixed(800.), false, None);
    let normal = scrolling_tile(2, &space, options);
    let normal_client = normal.window().clone();
    space.add_tile_to_column(0, None, normal, true);
    settle(&mut space);
    let spine = space.tiles().find(|tile| tile.window().id() == &1).unwrap();
    assert_eq!(spine.tile_size(), Size::from((800., 40.)));
    assert_eq!(normal_client.size(), Size::from((800, 632)));
    assert_eq!(spine.window().requested_size(), None);
}

#[test]
fn hidden_client_commit_cannot_resize_spine_outer_geometry() {
    let options = Rc::default();
    let mut tile = tile(1, 1., options);
    assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
    tile.request_tile_size((100., 200.).into(), false, None);
    tile.window().0.forced_size.set(Some((1500, 1200).into()));
    tile.window().communicate();
    tile.update_window();
    assert_eq!(tile.window_size(), Size::from((1500., 1200.)));
    assert_eq!(tile.tile_size(), Size::from((40., 200.)));
    assert!(!tile.are_transitions_ongoing());
}

#[test]
fn native_floating_insertion_preserves_spine_without_client_resize() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let options = Rc::default();
        let mut tile = tile(1, 1., options);
        assert!(tile.set_spine(axis, 40.));
        let before = tile.tile_size();
        let client = tile.window().clone();
        let mut space = FloatingSpace::new(
            (1280., 720.).into(),
            Rectangle::from_size((1280., 720.).into()),
            1.,
            tile.clock.clone(),
            Rc::default(),
        );
        space.add_tile(tile, true);
        space.verify_invariants();
        assert_eq!(space.tiles().next().unwrap().tile_size(), before);
        assert_eq!(client.requested_size(), None);
        let removed = space.remove_tile(&1);
        assert_eq!(removed.tile.form(), WindowForm::Spine(axis));
        assert_eq!(removed.tile.floating_window_size, client.expected_size());
        assert_ne!(removed.tile.floating_window_size, Some((40, 40).into()));
        assert_eq!(removed.tile.tile_size(), before);
    }
}

#[test]
fn normal_geometry_and_client_constraints_keep_original_behavior() {
    let options: Rc<Options> = Rc::default();
    let mut space = scrolling(1., options.clone());
    let normal = scrolling_tile(1, &space, options.clone());
    let client = normal.window().clone();
    space.add_tile(None, normal, true, ColumnWidth::Fixed(400.), false, None);
    settle(&mut space);
    assert_eq!(client.size().w, 600);
    let normal = scrolling_tile(2, &space, options);
    space.add_tile_to_column(0, None, normal, true);
    settle(&mut space);
    for tile in space.tiles() {
        assert_eq!(tile.form(), WindowForm::Normal);
        assert_eq!(tile.window().requested_size().unwrap().w, 600);
        assert_eq!(tile.tile_size().w, 600.);
    }
}

#[test]
fn fullscreen_and_invalid_thickness_reject_without_partial_conversion() {
    let mut tile = tile(1, 1., Rc::default());
    assert!(!tile.set_spine(ShrinkAxis::Horizontal, f64::NAN));
    assert_eq!(tile.form(), WindowForm::Normal);
    tile.window()
        .0
        .pending_sizing_mode
        .set(SizingMode::Fullscreen);
    assert!(!tile.set_spine(ShrinkAxis::Horizontal, 40.));
    assert_eq!(tile.form(), WindowForm::Normal);
}

#[test]
fn tiny_output_uses_native_minimum_overflow_without_changing_spine_axis() {
    let options: Rc<Options> = Rc::default();
    let mut space = ScrollingSpace::new(
        (20., 20.).into(),
        Rectangle::from_size((20., 20.).into()),
        1.,
        Clock::with_time(Duration::ZERO),
        options.clone(),
    );
    for (id, axis) in [(1, ShrinkAxis::Horizontal), (2, ShrinkAxis::Vertical)] {
        let mut tile = scrolling_tile(id, &space, options.clone());
        assert!(tile.set_spine(axis, 40.));
        if id == 1 {
            space.add_tile(None, tile, true, ColumnWidth::Fixed(400.), false, None);
        } else {
            space.add_tile_to_column(0, None, tile, false);
        }
    }
    settle(&mut space);
    assert_eq!(space.columns().count(), 1);
    for (tile, axis) in space
        .tiles()
        .zip([ShrinkAxis::Horizontal, ShrinkAxis::Vertical])
    {
        assert_eq!(tile.form(), WindowForm::Spine(axis));
        assert!(tile.tile_size().w >= 40.);
        assert!(tile.tile_size().h >= 40.);
        assert_eq!(tile.window().requested_size(), None);
    }
}

#[test]
fn output_scale_and_border_reload_reallocate_spines_without_client_configure() {
    let options: Rc<Options> = Rc::default();
    let mut space = scrolling(1., options.clone());
    let mut tile = scrolling_tile(1, &space, options);
    assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
    space.add_tile(None, tile, true, ColumnWidth::Fixed(400.), false, None);
    let mut options = Options::default();
    options.layout.border.off = false;
    options.layout.border.width = 30.;
    space.update_config(
        (1280., 720.).into(),
        Rectangle::from_size((1280., 720.).into()),
        1.25,
        Rc::new(options),
    );
    settle(&mut space);
    let tile = space.tiles().next().unwrap();
    assert_eq!(tile.form(), WindowForm::Spine(ShrinkAxis::Horizontal));
    // Native rounding turns a 30 logical-pixel border into 38 physical pixels per side.
    // Together with one interior pixel, that is 77 / 1.25 = 61.6 logical pixels.
    assert_eq!(tile.tile_size().w, 61.6);
    assert_eq!(tile.window().requested_size(), None);
}

#[test]
fn native_column_consumption_keeps_spine_identity_and_axis() {
    let options: Rc<Options> = Rc::default();
    let mut space = scrolling(1., options.clone());
    let mut clients = Vec::new();
    for id in 1..=2 {
        let mut tile = scrolling_tile(id, &space, options.clone());
        clients.push(tile.window().clone());
        assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
        space.add_tile(
            Some(id - 1),
            tile,
            id == 1,
            ColumnWidth::Fixed(400.),
            false,
            None,
        );
    }
    space.consume_into_column();
    settle(&mut space);
    assert_eq!(space.columns().count(), 1);
    for (tile, client) in space.tiles().zip(&clients) {
        assert!(Rc::ptr_eq(&tile.window().0, &client.0));
        assert_eq!(tile.form(), WindowForm::Spine(ShrinkAxis::Horizontal));
        assert_eq!(tile.tile_size().w, 40.);
        assert_eq!(client.requested_size(), None);
    }
}

#[test]
fn spine_sizing_updates_column_geometry_without_waiting_for_client_commit() {
    let options: Rc<Options> = Rc::default();
    let mut space = scrolling(1., options.clone());
    let mut tile = scrolling_tile(1, &space, options);
    assert!(tile.set_spine(ShrinkAxis::Vertical, 40.));
    space.add_tile(None, tile, true, ColumnWidth::Fixed(400.), false, None);
    space.set_window_width(Some(&1), SizeChange::SetFixed(650));
    assert_eq!(space.tiles().next().unwrap().tile_size().w, 650.);
    assert_eq!(
        space.tiles().next().unwrap().window().requested_size(),
        None
    );
    // There is no client configure/commit that could fix stale ColumnData afterwards.
    space.verify_invariants();
    space.toggle_full_width();
    space.verify_invariants();
    space.toggle_window_width(Some(&1), true);
    space.verify_invariants();
    space.expand_column_to_available_width();
    space.verify_invariants();
}

#[test]
fn spine_sizing_height_uses_outer_geometry_and_ignores_client_minimum() {
    let options: Rc<Options> = Rc::default();
    let mut space = scrolling(1., options.clone());
    let mut tile = scrolling_tile(1, &space, options);
    assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
    space.add_tile(None, tile, true, ColumnWidth::Fixed(400.), false, None);
    let initial = space.tiles().next().unwrap().tile_size().h;
    space.set_window_height(Some(&1), SizeChange::AdjustFixed(-500));
    assert_eq!(space.tiles().next().unwrap().tile_size().h, initial - 500.);
    assert_eq!(space.tiles().next().unwrap().tile_size().w, 40.);
    assert_eq!(
        space.tiles().next().unwrap().window().requested_size(),
        None
    );
    space.verify_invariants();
}

#[test]
fn spine_sizing_floating_uses_outer_geometry_and_does_not_configure_client() {
    let mut tile = tile(1, 1., Rc::default());
    assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
    let mut space = FloatingSpace::new(
        (1280., 720.).into(),
        Rectangle::from_size((1280., 720.).into()),
        1.,
        tile.clock.clone(),
        Rc::default(),
    );
    space.add_tile(tile, true);
    let initial = space.tiles().next().unwrap().tile_size().h;
    space.set_window_height(Some(&1), SizeChange::AdjustFixed(-500), false);
    let tile = space.tiles().next().unwrap();
    assert_eq!(tile.tile_size().h, initial - 500.);
    assert_eq!(tile.tile_size().w, 40.);
    assert_eq!(tile.window().requested_size(), None);
    space.verify_invariants();
}

#[test]
fn ordinary_navigation_skips_spine_columns_and_hidden_tabs() {
    for display in [ColumnDisplay::Normal, ColumnDisplay::Tabbed] {
        let mut options = Options::default();
        options.layout.default_column_display = display;
        let options = Rc::new(options);
        let mut space = scrolling(1., options.clone());
        for id in 1..=5 {
            let mut tile = Tile::new(
                TestWindow::new(TestWindowParams::new(id)),
                (1280., 720.).into(),
                1.,
                space.clock().clone(),
                options.clone(),
            );
            if id == 2 || id == 4 {
                assert!(tile.set_spine(ShrinkAxis::Vertical, 40.));
            }
            if id <= 3 {
                if id == 1 {
                    space.add_tile(None, tile, true, ColumnWidth::Fixed(400.), false, None);
                } else {
                    space.add_tile_to_column(0, None, tile, false);
                }
            } else {
                space.add_tile(None, tile, false, ColumnWidth::Fixed(400.), false, None);
            }
        }
        settle(&mut space);
        assert!(space.activate_window(&1));
        assert!(space.focus_down());
        assert_eq!(space.active_window().unwrap().id(), &3);
        assert!(space.focus_up());
        assert_eq!(space.active_window().unwrap().id(), &1);
        space.focus_window_in_column(2);
        assert_eq!(space.active_window().unwrap().id(), &3);
        assert!(space.focus_right());
        assert_eq!(space.active_window().unwrap().id(), &5);
        assert!(space.focus_left());
        assert_eq!(space.active_window().unwrap().id(), &3);
        // Native internal selection may still select a spine, without expanding it.
        assert!(space.activate_window(&2));
        space.refresh(true, true);
        assert!(!space
            .tiles()
            .find(|t| t.window().id() == &2)
            .unwrap()
            .window()
            .0
            .pending_activated
            .get());
        space.focus_column_first();
        assert_eq!(space.active_window().unwrap().id(), &1);
        space.focus_column(2);
        assert_eq!(space.active_window().unwrap().id(), &5);
        assert_eq!(space.tiles().count(), 5);
        assert_eq!(space.tiles().filter(|t| t.is_spine()).count(), 2);
        space.verify_invariants();
    }
}

#[test]
fn all_spine_workspace_keeps_inventory_without_client_focus_or_sink_conversion() {
    for floating in [false, true] {
        let mut workspace =
            Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::default());
        let window = TestWindow::new(TestWindowParams::new(1));
        let mut tile = workspace.make_tile(window.clone());
        assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
        workspace.add_tile(
            tile,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            ColumnWidth::Fixed(400.),
            false,
            floating,
            None,
        );
        workspace.refresh(true, true);
        assert_eq!(workspace.selected_window().unwrap().id(), &1);
        assert!(workspace.active_window().is_none());
        assert!(!window.0.pending_activated.get());
        assert!(workspace.has_windows_or_name());
        assert!(workspace.has_window(&1));
        assert_eq!(workspace.windows().count(), 1);
        assert!(!workspace.focus_left());
        assert!(!workspace.focus_right());
        assert!(!workspace.focus_up());
        assert!(!workspace.focus_down());
        workspace.focus_column_first();
        workspace.focus_column_last();
        workspace.focus_window_top();
        workspace.focus_window_bottom();
        assert!(!workspace.sink_window(&1));
        assert_eq!(workspace.sink_count(), 0);
        assert_eq!(
            workspace.tiles().next().unwrap().form(),
            WindowForm::Spine(ShrinkAxis::Horizontal)
        );
        assert_eq!(window.requested_size(), None);
        workspace.verify_invariants(None);
    }
}

#[test]
fn floating_navigation_skips_spines_and_keeps_normal_distance_order() {
    let options: Rc<Options> = Rc::default();
    let clock = Clock::with_time(Duration::ZERO);
    let mut space = FloatingSpace::new(
        (1280., 720.).into(),
        Rectangle::from_size((1280., 720.).into()),
        1.,
        clock.clone(),
        options.clone(),
    );
    for id in 1..=3 {
        let mut tile = Tile::new(
            TestWindow::new(TestWindowParams::new(id)),
            (1280., 720.).into(),
            1.,
            clock.clone(),
            options.clone(),
        );
        if id == 2 {
            assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
        }
        space.add_tile(tile, id == 1);
        space.move_window(
            Some(&id),
            PositionChange::SetFixed(id as f64 * 200.),
            PositionChange::SetFixed(id as f64 * 100.),
            false,
        );
    }
    assert!(space.focus_right());
    assert_eq!(space.active_window().unwrap().id(), &3);
    assert!(space.focus_left());
    assert_eq!(space.active_window().unwrap().id(), &1);
    assert!(space.focus_down());
    assert_eq!(space.active_window().unwrap().id(), &3);
    space.focus_leftmost();
    assert_eq!(space.active_window().unwrap().id(), &1);
    space.focus_rightmost();
    assert_eq!(space.active_window().unwrap().id(), &3);
    space.activate_window(&2);
    space.refresh(true, true);
    assert!(!space
        .tiles()
        .find(|t| t.window().id() == &2)
        .unwrap()
        .window()
        .0
        .pending_activated
        .get());
    assert_eq!(space.tiles().count(), 3);
    space.verify_invariants();
}

#[test]
fn moving_spine_retains_native_owner_without_client_focus() {
    let mut layout = Layout::<TestWindow>::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    Op::Communicate(1).apply(&mut layout);
    let output = layout.outputs().next().unwrap().clone();
    let window = {
        let workspace = layout
            .workspaces_mut()
            .find(|ws| ws.has_window(&1))
            .unwrap();
        let tile = workspace
            .tiles_mut()
            .find(|t| t.window().id() == &1)
            .unwrap();
        assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
        tile.window().clone()
    };
    layout.update_window(&1, None);
    layout.refresh(true);
    assert!(layout.is_spine(&1));
    assert!(layout.focus().is_none());
    assert!(!layout.should_trigger_focus_follows_mouse_on(&1));
    assert!(layout.interactive_move_begin(1, &output, (25., 100.).into()));
    assert!(layout.interactive_move_update(&1, (300., 300.).into(), output, (325., 400.).into()));
    assert!(matches!(
        layout.interactive_move,
        Some(InteractiveMoveState::Moving(_))
    ));
    layout.refresh(true);
    assert!(layout.is_spine(&1));
    assert!(layout.focus_with_output().is_none());
    assert!(!layout.should_trigger_focus_follows_mouse_on(&1));
    assert!(!window.0.pending_activated.get());
    assert_eq!(layout.windows().count(), 1);
    layout.verify_invariants();
    layout.interactive_move_end(&1);
    layout.refresh(true);
    assert!(layout.is_spine(&1));
    assert!(layout.focus().is_none());
    let retained = layout.windows().find(|(_, w)| w.id() == &1).unwrap().1;
    assert!(Rc::ptr_eq(&retained.0, &window.0));
    layout.verify_invariants();
}

#[test]
fn native_move_anchor_tracks_spine_outer_geometry_and_preserves_normal_geometry() {
    for axis in [
        None,
        Some(ShrinkAxis::Horizontal),
        Some(ShrinkAxis::Vertical),
    ] {
        for floating in [false, true] {
            for scale in [1., 1.25] {
                for overview in [false, true] {
                    let mut options = Options::default();
                    options.layout.border.off = false;
                    options.layout.border.width = 7.3;
                    let mut layout = Layout::<TestWindow>::with_options(
                        Clock::with_time(Duration::ZERO),
                        options,
                    );
                    layout.clock.set_complete_instantly(true);
                    Op::AddScaledOutput {
                        id: 1,
                        scale,
                        layout_config: None,
                    }
                    .apply(&mut layout);
                    let mut params = TestWindowParams::new(1);
                    params.is_floating = floating;
                    Op::AddWindow { params }.apply(&mut layout);
                    Op::Communicate(1).apply(&mut layout);
                    let window = {
                        let ws = layout
                            .workspaces_mut()
                            .find(|ws| ws.has_window(&1))
                            .unwrap();
                        let tile = ws
                            .tiles_mut()
                            .find(|tile| tile.window().id() == &1)
                            .unwrap();
                        if let Some(axis) = axis {
                            assert!(tile.set_spine(axis, 40.3));
                        }
                        tile.window().clone()
                    };
                    layout.update_window(&1, None);
                    if overview {
                        layout.open_overview();
                    }
                    layout.advance_animations();
                    let output = layout.outputs().next().unwrap().clone();
                    let zoom = layout.overview_zoom();
                    let ratio = (0.23, 0.67);
                    let start = layout
                        .monitors()
                        .find_map(|mon| {
                            mon.workspaces_with_render_geo().find_map(|(ws, geo)| {
                                let (tile, pos, _) = ws
                                    .tiles_with_render_positions()
                                    .find(|(tile, _, _)| tile.window().id() == &1)?;
                                let (size, offset) = if axis.is_some() {
                                    (tile.tile_size(), Point::from((0., 0.)))
                                } else {
                                    (tile.window_size(), tile.window_loc())
                                };
                                Some(
                                    geo.loc
                                        + (pos
                                            + offset
                                            + Point::from((size.w * ratio.0, size.h * ratio.1)))
                                        .upscale(zoom),
                                )
                            })
                        })
                        .unwrap();
                    let requested = window.requested_size();
                    assert!(layout.interactive_move_begin(1, &output, start));
                    let Some(InteractiveMoveState::Starting {
                        pointer_ratio_within_window,
                        ..
                    }) = &layout.interactive_move
                    else {
                        panic!("expected native Starting owner")
                    };
                    approx::assert_abs_diff_eq!(
                        pointer_ratio_within_window.0,
                        ratio.0,
                        epsilon = 1e-9
                    );
                    approx::assert_abs_diff_eq!(
                        pointer_ratio_within_window.1,
                        ratio.1,
                        epsilon = 1e-9
                    );
                    let mut pointer = start + Point::from((255. * zoom, 0.));
                    assert!(layout.interactive_move_update(
                        &1,
                        (255. * zoom, 0.).into(),
                        output.clone(),
                        pointer
                    ));
                    if !floating {
                        assert!(matches!(
                            layout.interactive_move,
                            Some(InteractiveMoveState::Starting { .. })
                        ));
                        pointer.x += zoom;
                        assert!(layout.interactive_move_update(
                            &1,
                            (zoom, 0.).into(),
                            output,
                            pointer
                        ));
                    }
                    layout.advance_animations();
                    let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move else {
                        panic!("expected native Moving owner")
                    };
                    let (size, offset) = if axis.is_some() {
                        (move_.tile.tile_size(), Point::from((0., 0.)))
                    } else {
                        (move_.tile.window_size(), move_.tile.window_loc())
                    };
                    let anchor = move_.tile_render_location(zoom)
                        + (Point::from((size.w * ratio.0, size.h * ratio.1)) + offset
                            - move_.tile.render_offset())
                        .upscale(zoom);
                    approx::assert_abs_diff_eq!(anchor.x, pointer.x, epsilon = 0.5 / scale + 1e-9);
                    approx::assert_abs_diff_eq!(anchor.y, pointer.y, epsilon = 0.5 / scale + 1e-9);
                    if axis.is_some() {
                        let pose = move_.tile_render_location(zoom);
                        let size = move_.tile.tile_size();
                        window.0.forced_size.set(Some((1500, 1100).into()));
                        assert!(window.communicate());
                        layout.update_window(&1, None);
                        let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move
                        else {
                            unreachable!()
                        };
                        assert_eq!(move_.tile.tile_size(), size);
                        assert_eq!(move_.tile_render_location(zoom), pose);
                        assert_eq!(window.requested_size(), requested);
                    }
                    layout.verify_invariants();
                    layout.interactive_move_end(&1);
                    assert_eq!(layout.is_spine(&1), axis.is_some());
                    assert_eq!(layout.windows().count(), 1);
                    layout.verify_invariants();
                }
            }
        }
    }
}

#[test]
fn native_moving_floating_toggle_does_not_resize_spine_client() {
    for axis in [
        None,
        Some(ShrinkAxis::Horizontal),
        Some(ShrinkAxis::Vertical),
    ] {
        let mut layout = Layout::<TestWindow>::default();
        Op::AddOutput(1).apply(&mut layout);
        Op::AddWindow {
            params: TestWindowParams::new(1),
        }
        .apply(&mut layout);
        Op::Communicate(1).apply(&mut layout);
        let window = {
            let ws = layout
                .workspaces_mut()
                .find(|ws| ws.has_window(&1))
                .unwrap();
            let tile = ws
                .tiles_mut()
                .find(|tile| tile.window().id() == &1)
                .unwrap();
            if let Some(axis) = axis {
                assert!(tile.set_spine(axis, 40.));
            }
            tile.window().clone()
        };
        layout.update_window(&1, None);
        let output = layout.outputs().next().unwrap().clone();
        assert!(layout.interactive_move_begin(1, &output, (30., 100.).into()));
        assert!(layout.interactive_move_update(&1, (300., 0.).into(), output, (330., 100.).into()));
        let Some(InteractiveMoveState::Moving(move_)) = &mut layout.interactive_move else {
            unreachable!()
        };
        move_.tile.floating_window_size = Some((1111, 777).into());
        let before = window.requested_size();
        let outer = move_.tile.tile_size();
        layout.toggle_window_floating(Some(&1));
        let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move else {
            unreachable!()
        };
        assert!(move_.is_floating);
        if axis.is_some() {
            assert_eq!(window.requested_size(), before);
            assert_eq!(move_.tile.tile_size(), outer);
        } else {
            assert_eq!(window.requested_size(), Some((1111, 777).into()));
        }
        layout.interactive_move_end(&1);
        assert!(layout.active_workspace().unwrap().is_floating(&1));
        assert_eq!(layout.is_spine(&1), axis.is_some());
        let retained = layout.windows().find(|(_, w)| w.id() == &1).unwrap().1;
        assert!(Rc::ptr_eq(&retained.0, &window.0));
        layout.verify_invariants();
    }
}

#[test]
fn moving_book_mode_requests_do_not_partially_restore_or_change_client_intent() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let mut layout = Layout::<TestWindow>::default();
        Op::AddOutput(1).apply(&mut layout);
        Op::AddWindow {
            params: TestWindowParams::new(1),
        }
        .apply(&mut layout);
        Op::Communicate(1).apply(&mut layout);
        let ws = layout
            .workspaces_mut()
            .find(|ws| ws.has_window(&1))
            .unwrap();
        let tile = ws
            .tiles_mut()
            .find(|tile| tile.window().id() == &1)
            .unwrap();
        assert!(tile.set_spine(axis, 40.));
        let window = tile.window().clone();
        layout.update_window(&1, None);
        let output = layout.outputs().next().unwrap().clone();
        assert!(layout.interactive_move_begin(1, &output, (30., 100.).into()));
        assert!(layout.interactive_move_update(&1, (300., 0.).into(), output, (330., 100.).into()));
        let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move else {
            unreachable!()
        };
        let size = move_.tile.tile_size();
        let requested = window.requested_size();
        layout.request_fullscreen(&1, true);
        layout.request_maximized(&1, true);
        layout.request_fullscreen(&1, false);
        layout.request_maximized(&1, false);
        layout.request_toggle_fullscreen(&1);
        layout.request_toggle_maximized(&1);
        layout.request_toggle_windowed_fullscreen(&1);
        let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move else {
            unreachable!()
        };
        assert_eq!(move_.tile.form(), WindowForm::Spine(axis));
        assert_eq!(move_.tile.tile_size(), size);
        assert!(!move_.tile.has_normal_restore_allocation());
        assert_eq!(window.requested_size(), requested);
        assert_eq!(window.pending_sizing_mode(), SizingMode::Normal);
        assert!(!window.is_pending_windowed_fullscreen());
        assert_eq!(layout.windows().count(), 1);
        layout.verify_invariants();
        // The final virtual intent is windowed fullscreen with latent maximize.
        layout.interactive_move_end(&1);
        assert!(!layout.is_spine(&1));
        assert_eq!(window.pending_sizing_mode(), SizingMode::Maximized);
        assert!(window.is_pending_windowed_fullscreen());
        layout.verify_invariants();
    }
}

#[test]
fn spine_is_not_a_cursor_warp_target_in_either_native_space() {
    for floating in [false, true] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            let mut layout = Layout::<TestWindow>::default();
            Op::AddOutput(1).apply(&mut layout);
            let mut params = TestWindowParams::new(1);
            params.is_floating = floating;
            Op::AddWindow { params }.apply(&mut layout);
            Op::Communicate(1).apply(&mut layout);
            let ws = layout
                .workspaces_mut()
                .find(|ws| ws.has_window(&1))
                .unwrap();
            assert!(ws.active_window_visual_rectangle().is_some());
            assert!(ws
                .tiles_mut()
                .find(|tile| tile.window().id() == &1)
                .unwrap()
                .set_spine(axis, 40.));
            ws.update_window(&1, None);
            assert!(ws.active_window().is_none());
            assert!(ws.active_window_visual_rectangle().is_none());
            // This internal form change is a control, not a user Expand action.
            assert!(ws
                .tiles_mut()
                .find(|tile| tile.window().id() == &1)
                .unwrap()
                .clear_spine());
            ws.update_window(&1, None);
            assert!(ws.active_window_visual_rectangle().is_some());
        }
    }
}

#[test]
fn spine_hit_is_independent_of_client_input_and_preserves_body_and_edge_identity() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        for scale in [1., 1.25, 2.] {
            let mut tile = tile(1, scale, Rc::default());
            assert!(tile.set_spine(axis, 40.));
            tile.request_tile_size((400., 280.).into(), false, None);
            let size = tile.tile_size();
            let center = size.to_point().downscale(2.);
            assert_eq!(
                tile.hit(center),
                Some(HitType::Spine {
                    edges: ResizeEdge::empty()
                })
            );
            assert_eq!(
                tile.hit((0.5, 0.5).into()),
                Some(HitType::Spine {
                    edges: ResizeEdge::TOP_LEFT
                })
            );
            assert_eq!(
                tile.hit((size.w - 0.5, size.h - 0.5).into()),
                Some(HitType::Spine {
                    edges: ResizeEdge::BOTTOM_RIGHT
                })
            );
            assert!(tile.hit((-1., center.y).into()).is_none());
            assert!(tile.hit((size.w + 1., center.y).into()).is_none());
            let hit = HitType::hit_tile(
                &tile,
                (100., 200.).into(),
                center + Point::from((100., 200.)),
            )
            .unwrap()
            .1;
            assert_eq!(
                hit.to_activate(),
                HitType::Spine {
                    edges: ResizeEdge::empty()
                }
            );
        }
    }
}

#[test]
fn shrink_behavior_reload_does_not_resize_or_defocus_normal_windows() {
    for floating in [false, true] {
        let mut layout = Layout::<TestWindow>::default();
        Op::AddOutput(1).apply(&mut layout);
        let mut params = TestWindowParams::new(1);
        params.is_floating = floating;
        Op::AddWindow { params }.apply(&mut layout);
        Op::Communicate(1).apply(&mut layout);
        layout.refresh(true);
        let before = {
            let tile = layout
                .workspaces()
                .flat_map(|(_, _, ws)| ws.tiles())
                .next()
                .unwrap();
            (tile.tile_size(), tile.window().requested_size())
        };
        let config =
            niri_config::Config::parse_mem("shrink-expand { enabled true; thickness 72; }")
                .unwrap();
        layout.update_config(&config);
        layout.refresh(true);
        let tile = layout
            .workspaces()
            .flat_map(|(_, _, ws)| ws.tiles())
            .next()
            .unwrap();
        assert_eq!((tile.tile_size(), tile.window().requested_size()), before);
        assert_eq!(tile.form(), WindowForm::Normal);
        assert_eq!(layout.focus().unwrap().id(), &1);
        layout.verify_invariants();
    }
}

#[test]
fn spine_thickness_reload_reflows_native_slots_without_resizing_hidden_clients() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        for floating in [false, true] {
            let mut layout = Layout::<TestWindow>::default();
            Op::AddOutput(1).apply(&mut layout);
            let mut params = TestWindowParams::new(1);
            params.is_floating = floating;
            Op::AddWindow { params }.apply(&mut layout);
            Op::Communicate(1).apply(&mut layout);
            let window = {
                let tile = layout
                    .workspaces_mut()
                    .find(|ws| ws.has_window(&1))
                    .unwrap()
                    .tiles_mut()
                    .find(|t| t.window().id() == &1)
                    .unwrap();
                assert!(tile.set_spine(axis, 40.));
                tile.window().clone()
            };
            layout.update_window(&1, None);
            layout.refresh(true);
            let requested = window.requested_size();
            let config = niri_config::Config::parse_mem(
                "shrink-expand { enabled true; thickness 64; shrink-buffer-ms 900; }",
            )
            .unwrap();
            layout.update_config(&config);
            layout.refresh(true);
            assert_eq!(layout.options.shrink_expand, config.shrink_expand);
            let book = layout
                .workspaces()
                .flat_map(|(_, _, ws)| ws.tiles())
                .find(|t| t.window().id() == &1)
                .unwrap();
            assert_eq!(book.form(), WindowForm::Spine(axis));
            let size = book.tile_size();
            assert_eq!(
                match axis {
                    ShrinkAxis::Horizontal => size.w,
                    ShrinkAxis::Vertical => size.h,
                },
                64.
            );
            assert!(size.w >= 64. && size.h >= 64.);
            assert_eq!(window.requested_size(), requested);
            assert!(layout.focus().is_none());
            layout.verify_invariants();
            // A paint-only reload must keep an explicitly constructed book thickness.
            let mut config = config;
            config.layout.spine.inactive = niri_config::SpinePaint::Solid("#246".parse().unwrap());
            layout.update_config(&config);
            layout.refresh(true);
            assert_eq!(window.requested_size(), requested);
            assert!(layout.is_spine(&1));
            layout.verify_invariants();
            Op::RemoveOutput(1).apply(&mut layout);
            config.shrink_expand.thickness = 96.;
            layout.update_config(&config);
            let book = layout
                .workspaces()
                .flat_map(|(_, _, ws)| ws.tiles())
                .find(|t| t.window().id() == &1)
                .unwrap();
            let size = book.tile_size();
            assert_eq!(
                match axis {
                    ShrinkAxis::Horizontal => size.w,
                    ShrinkAxis::Vertical => size.h,
                },
                96.
            );
            assert_eq!(window.requested_size(), requested);
            layout.verify_invariants();
            Op::AddOutput(1).apply(&mut layout);
            layout.refresh(true);
            assert!(layout.is_spine(&1));
            layout.verify_invariants();
        }
    }
}

#[test]
fn moving_spine_thickness_reload_preserves_owner_axis_and_hidden_client_request() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let mut layout = Layout::<TestWindow>::default();
        Op::AddOutput(1).apply(&mut layout);
        Op::AddWindow {
            params: TestWindowParams::new(1),
        }
        .apply(&mut layout);
        Op::Communicate(1).apply(&mut layout);
        let window = {
            let tile = layout
                .workspaces_mut()
                .find(|ws| ws.has_window(&1))
                .unwrap()
                .tiles_mut()
                .find(|t| t.window().id() == &1)
                .unwrap();
            assert!(tile.set_spine(axis, 40.));
            tile.window().clone()
        };
        layout.update_window(&1, None);
        let output = layout.outputs().next().unwrap().clone();
        assert!(layout.interactive_move_begin(1, &output, (30., 100.).into()));
        assert!(layout.interactive_move_update(&1, (300., 0.).into(), output, (330., 100.).into()));
        let before = window.requested_size();
        let config =
            niri_config::Config::parse_mem("shrink-expand { enabled true; thickness 72; }")
                .unwrap();
        layout.update_config(&config);
        let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move else {
            unreachable!()
        };
        assert_eq!(move_.tile.form(), WindowForm::Spine(axis));
        let size = move_.tile.tile_size();
        assert_eq!(
            match axis {
                ShrinkAxis::Horizontal => size.w,
                ShrinkAxis::Vertical => size.h,
            },
            72.
        );
        assert_eq!(window.requested_size(), before);
        assert_eq!(layout.windows().count(), 1);
        layout.verify_invariants();
        layout.interactive_move_end(&1);
        layout.refresh(true);
        assert!(layout.is_spine(&1));
        assert_eq!(window.requested_size(), before);
        layout.verify_invariants();
    }
}

#[test]
fn spine_rule_update_requantizes_border_without_configuring_hidden_client() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        for scale in [1., 1.25, 2.] {
            let mut book = tile(1, scale, Rc::default());
            assert!(book.set_spine(axis, 40.));
            book.request_tile_size((400., 280.).into(), false, None);
            let before = book.tile_size();
            let rules = &mut Rc::get_mut(&mut book.window_mut().0).unwrap().rules;
            rules.border.on = true;
            rules.border.width = Some(niri_config::FloatOrInt(30.));
            book.update_window();
            let size = book.tile_size();
            let thickness = match axis {
                ShrinkAxis::Horizontal => size.w,
                ShrinkAxis::Vertical => size.h,
            };
            assert!(thickness > 60.);
            assert!(size.w >= thickness && size.h >= thickness);
            assert_eq!(book.window().requested_size(), None);
            let rules = &mut Rc::get_mut(&mut book.window_mut().0).unwrap().rules;
            rules.border.on = false;
            rules.border.off = true;
            book.update_window();
            assert_eq!(book.tile_size(), before);
            assert_eq!(book.window().requested_size(), None);
        }
    }
}

fn spine_test_renderer() -> smithay::backend::renderer::gles::GlesRenderer {
    use crate::render_helpers::{resources, shaders};
    use smithay::backend::egl::native::EGLSurfacelessDisplay;
    use smithay::backend::egl::{EGLContext, EGLDisplay};
    use smithay::backend::renderer::gles::GlesRenderer;
    let mut renderer = unsafe {
        let display = EGLDisplay::new(EGLSurfacelessDisplay).unwrap();
        let context = EGLContext::new(&display).unwrap();
        GlesRenderer::new(context).unwrap()
    };
    resources::init(&mut renderer);
    shaders::init(&mut renderer);
    renderer
}

#[test]
#[ignore = "requires explicit software EGL committed-shrink rendering"]
fn early_shrink_commit_and_immediate_expand_preserve_the_visible_pose() {
    use crate::layout::shrink_expand::NormalSizePlan;
    use crate::render_helpers::solid_color::SolidColorBuffer;
    use crate::render_helpers::{render_and_download, RenderTarget};
    use smithay::backend::allocator::Fourcc;
    use smithay::backend::renderer::element::Element as _;
    use smithay::backend::renderer::ExportMem as _;
    let mut renderer = spine_test_renderer();
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        for anchor in [(0., 0.), (1., 1.)] {
            for scale in [1., 1.25] {
                let mut options = Options::default();
                options.layout.border.off = true;
                options.layout.focus_ring.off = true;
                options.layout.spine.inactive =
                    niri_config::SpinePaint::Solid("#ff0000".parse().unwrap());
                let config = niri_config::Animation {
                    off: false,
                    kind: niri_config::animations::Kind::Easing(
                        niri_config::animations::EasingParams {
                            duration_ms: 200,
                            curve: niri_config::animations::Curve::Linear,
                        },
                    ),
                };
                options.animations.window_movement.0 = config;
                options.animations.window_resize.anim = config;
                let mut params = TestWindowParams::new(1);
                params.bbox = Rectangle::from_size((300, 200).into());
                params.rules = Some(ResolvedWindowRules {
                    block_out_from: Some(niri_config::BlockOutFrom::ScreenCapture),
                    ..Default::default()
                });
                let client = TestWindow::new(params);
                *client.0.render_solid.borrow_mut() =
                    Some(SolidColorBuffer::new((300., 200.), [0., 1., 0., 1.]));
                let mut clock = Clock::with_time(Duration::ZERO);
                let mut tile = Tile::new(
                    client.clone(),
                    (1280., 720.).into(),
                    scale,
                    clock.clone(),
                    Rc::new(options),
                );
                let target = if axis == ShrinkAxis::Horizontal {
                    (40., 200.)
                } else {
                    (300., 40.)
                };
                tile.set_shrink_preview(target.into(), true);
                tile.set_shrink_preview_anchor(anchor.into());
                clock.set_unadjusted(Duration::from_millis(50));
                tile.update_render_elements(false, Rectangle::from_size((1280., 720.).into()));
                let collect = |tile: &Tile<TestWindow>, renderer: &mut _, target| {
                    let mut elements = Vec::new();
                    tile.render(
                        RenderCtx {
                            renderer,
                            target,
                            xray: None,
                        },
                        (0., 0.).into(),
                        XrayPos::default(),
                        false,
                        &mut |elem| elements.push(elem),
                    );
                    elements
                };
                let before = collect(&tile, &mut renderer, RenderTarget::Output);
                let before_ghost = before[0].geometry(Scale::from(scale));
                assert!(tile.set_spine(axis, 40.));
                assert_eq!(tile.tile_size(), Size::from(target));
                assert_eq!(tile.form(), WindowForm::Spine(axis));
                assert!(tile.are_transitions_ongoing());
                assert_eq!(client.requested_size(), None);
                let normal_calls = client.0.render_normal_calls.get();
                let popup_calls = client.0.render_popup_calls.get();
                tile.update_render_elements(false, Rectangle::from_size((1280., 720.).into()));
                let committed = collect(&tile, &mut renderer, RenderTarget::Output);
                assert_eq!(committed[0].geometry(Scale::from(scale)), before_ghost);
                assert_eq!(client.0.render_normal_calls.get(), normal_calls);
                assert_eq!(client.0.render_popup_calls.get(), popup_calls);
                let blocked = collect(&tile, &mut renderer, RenderTarget::ScreenCapture);
                let physical = Size::from((300., 200.)).to_physical_precise_round(scale);
                let download = render_and_download(
                    &mut renderer,
                    physical,
                    Scale::from(scale),
                    Transform::Normal,
                    Fourcc::Abgr8888,
                    blocked.into_iter().rev(),
                )
                .unwrap();
                assert!(renderer
                    .map_texture(&download)
                    .unwrap()
                    .chunks_exact(4)
                    .all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0));
                clock.set_unadjusted(Duration::from_millis(100));
                tile.advance_animations();
                tile.update_render_elements(false, Rectangle::from_size((1280., 720.).into()));
                let shrinking = collect(&tile, &mut renderer, RenderTarget::Output);
                let shrinking_ghost = shrinking[0].geometry(Scale::from(scale));
                assert_ne!(shrinking_ghost, before_ghost);
                // Reversing before the visual finishes must continue from what was displayed.
                let plan = NormalSizePlan {
                    allocation: (600., 400.).into(),
                    client_size: (600, 400).into(),
                    sizing_mode: SizingMode::Normal,
                };
                assert!(tile.prepare_normal_restore(plan));
                tile.submit_normal_restore(false, Transaction::new(), false);
                tile.update_render_elements(false, Rectangle::from_size((1280., 720.).into()));
                let reversing = collect(&tile, &mut renderer, RenderTarget::Output);
                assert_eq!(reversing[0].geometry(Scale::from(scale)), shrinking_ghost);
                assert_eq!(tile.tile_size(), plan.allocation);
                assert!(tile.normal_restore_input_blocked());
                assert_eq!(client.0.render_normal_calls.get(), normal_calls);
                assert_eq!(client.0.render_popup_calls.get(), popup_calls);
            }
        }
    }
}

#[test]
#[ignore = "requires an explicit software EGL restoration rendering run"]
fn normal_restore_presentation_hides_stale_content_and_releases_without_input_delay() {
    use crate::layout::shrink_expand::NormalSizePlan;
    use crate::render_helpers::solid_color::SolidColorBuffer;
    use crate::render_helpers::{render_and_download, RenderTarget};
    use smithay::backend::allocator::Fourcc;
    use smithay::backend::renderer::ExportMem as _;
    let mut renderer = spine_test_renderer();
    for scale in [1., 1.25] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            for animate in [false, true] {
                let mut options = Options::default();
                options.layout.border.off = true;
                options.layout.focus_ring.off = true;
                options.layout.spine.inactive =
                    niri_config::SpinePaint::Solid(niri_config::Color::from_array_unpremul([
                        1., 0., 0., 0.5,
                    ]));
                options.shrink_expand.animate = animate;
                options.animations.window_resize.anim = niri_config::Animation {
                    off: false,
                    kind: niri_config::animations::Kind::Easing(
                        niri_config::animations::EasingParams {
                            duration_ms: 200,
                            curve: niri_config::animations::Curve::Linear,
                        },
                    ),
                };
                let mut params = TestWindowParams::new(1);
                params.bbox = Rectangle::from_size((120, 80).into());
                params.rules = Some(ResolvedWindowRules {
                    block_out_from: Some(niri_config::BlockOutFrom::ScreenCapture),
                    ..Default::default()
                });
                let client = TestWindow::new(params);
                *client.0.render_solid.borrow_mut() =
                    Some(SolidColorBuffer::new((240., 160.), [0., 1., 0., 1.]));
                *client.0.render_solid_blocked.borrow_mut() =
                    Some(SolidColorBuffer::new((240., 160.), [0., 0., 0., 1.]));
                let mut clock = Clock::with_time(Duration::ZERO);
                let mut tile = Tile::new(
                    client.clone(),
                    (1280., 720.).into(),
                    scale,
                    clock.clone(),
                    Rc::new(options),
                );
                assert!(tile.set_spine(axis, 40.));
                let book_size = tile.tile_size();
                let plan = NormalSizePlan {
                    allocation: (240., 160.).into(),
                    client_size: (240, 160).into(),
                    sizing_mode: SizingMode::Normal,
                };
                assert!(tile.prepare_normal_restore(plan));
                tile.set_normal_restore_anchor((1., 1.).into());
                let transaction = Transaction::new();
                let deadline = transaction.deadline();
                tile.submit_normal_restore(false, transaction, false);
                tile.update_render_elements(false, Rectangle::from_size((1280., 720.).into()));
                let collect = |tile: &Tile<TestWindow>, renderer: &mut _, target| {
                    let mut elements = Vec::new();
                    tile.render(
                        RenderCtx {
                            renderer,
                            target,
                            xray: None,
                        },
                        (0., 0.).into(),
                        XrayPos::default(),
                        false,
                        &mut |elem| elements.push(elem),
                    );
                    elements
                };
                let initial = collect(&tile, &mut renderer, RenderTarget::Output);
                let physical = plan.allocation.to_physical_precise_round(scale);
                let download = render_and_download(
                    &mut renderer,
                    physical,
                    Scale::from(scale),
                    Transform::Normal,
                    Fourcc::Abgr8888,
                    initial.into_iter().rev(),
                )
                .unwrap();
                let pixels = renderer.map_texture(&download).unwrap();
                // Growing from a left/top resize retains the bottom/right edge.
                let x = (5. * scale) as usize;
                let y = (5. * scale) as usize;
                let corner = &pixels[(y * physical.w as usize + x) * 4..][..4];
                if animate {
                    assert_eq!(corner, [0, 0, 0, 0]);
                }
                let x = ((plan.allocation.w - book_size.w / 2.) * scale) as usize;
                let y = ((plan.allocation.h - book_size.h / 2.) * scale) as usize;
                let p = &pixels[(y * physical.w as usize + x) * 4..][..4];
                assert!(
                    p[0].abs_diff(128) <= 1 && p[1] == 0 && p[3].abs_diff(128) <= 1,
                    "{p:?}"
                );
                assert_eq!(client.0.render_normal_calls.get(), 0);
                assert_eq!(client.0.render_popup_calls.get(), 0);
                assert!(tile.normal_restore_input_blocked());
                assert_eq!(client.normal_restore_deadline(), Some(deadline));
                // Completing the visual motion cannot release the protocol barrier.
                clock.set_unadjusted(Duration::from_millis(250));
                tile.advance_animations();
                tile.update_render_elements(false, Rectangle::from_size((1280., 720.).into()));
                let waiting = collect(&tile, &mut renderer, RenderTarget::Output);
                assert!(tile.normal_restore_input_blocked());
                assert_eq!(tile.tile_size(), plan.allocation);
                assert_eq!(client.0.render_normal_calls.get(), 0);
                drop(waiting);
                assert!(client.communicate());
                tile.update_window();
                assert!(!tile.normal_restore_input_blocked());
                assert_eq!(client.normal_restore_deadline(), None);
                clock.set_unadjusted(Duration::from_millis(350));
                tile.advance_animations();
                tile.update_render_elements(false, Rectangle::from_size((1280., 720.).into()));
                let output = collect(&tile, &mut renderer, RenderTarget::Output);
                let feedback = client
                    .0
                    .offscreen_data
                    .borrow()
                    .as_ref()
                    .map(|data| data.id.clone());
                let capture = collect(&tile, &mut renderer, RenderTarget::ScreenCapture);
                if animate {
                    assert!(feedback.is_some());
                    assert_eq!(
                        client
                            .0
                            .offscreen_data
                            .borrow()
                            .as_ref()
                            .map(|data| data.id.clone()),
                        feedback
                    );
                }
                let download = render_and_download(
                    &mut renderer,
                    physical,
                    Scale::from(scale),
                    Transform::Normal,
                    Fourcc::Abgr8888,
                    output.into_iter().rev(),
                )
                .unwrap();
                let pixels = renderer.map_texture(&download).unwrap();
                assert!(
                    pixels.chunks_exact(4).all(|p| p[1] > 0),
                    "ready contents must be visible"
                );
                if animate {
                    assert!(pixels.chunks_exact(4).all(|p| p[0] > 0 && p[1] < 255));
                }
                let download = render_and_download(
                    &mut renderer,
                    physical,
                    Scale::from(scale),
                    Transform::Normal,
                    Fourcc::Abgr8888,
                    capture.into_iter().rev(),
                )
                .unwrap();
                assert!(renderer
                    .map_texture(&download)
                    .unwrap()
                    .chunks_exact(4)
                    .all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0));
                clock.set_unadjusted(Duration::from_millis(500));
                tile.advance_animations();
                assert!(!tile.are_transitions_ongoing());
                let settled = collect(&tile, &mut renderer, RenderTarget::Output);
                assert!(client.0.offscreen_data.borrow().is_none());
                let download = render_and_download(
                    &mut renderer,
                    physical,
                    Scale::from(scale),
                    Transform::Normal,
                    Fourcc::Abgr8888,
                    settled.into_iter().rev(),
                )
                .unwrap();
                assert!(renderer
                    .map_texture(&download)
                    .unwrap()
                    .chunks_exact(4)
                    .all(|p| p == [0, 255, 0, 255]));
            }
        }
    }
}

#[test]
#[ignore = "requires an explicit software EGL preview rendering run"]
fn shrink_preview_gray_source_constant_gradient_and_capture_cache_are_independent() {
    use crate::render_helpers::solid_color::SolidColorBuffer;
    use crate::render_helpers::{render_and_download, RenderTarget};
    use smithay::backend::allocator::Fourcc;
    use smithay::backend::renderer::element::Element as _;
    use smithay::backend::renderer::ExportMem as _;
    let mut renderer = spine_test_renderer();
    for scale in [1., 1.25] {
        let mut options = Options::default();
        options.layout.border.off = true;
        options.layout.focus_ring.off = true;
        options.animations.off = true;
        options.layout.spine.inactive = niri_config::SpinePaint::Gradient(niri_config::Gradient {
            from: "#ff0000".parse().unwrap(),
            to: "#0000ff".parse().unwrap(),
            angle: 90,
            relative_to: niri_config::GradientRelativeTo::WorkspaceView,
            in_: niri_config::GradientInterpolation {
                color_space: niri_config::GradientColorSpace::SrgbLinear,
                ..Default::default()
            },
        });
        let mut params = TestWindowParams::new(1);
        params.bbox = Rectangle::from_size((120, 80).into());
        params.rules = Some(ResolvedWindowRules {
            block_out_from: Some(niri_config::BlockOutFrom::ScreenCapture),
            ..Default::default()
        });
        let mut window = Tile::new(
            TestWindow::new(params),
            (1280., 720.).into(),
            scale,
            Clock::with_time(Duration::ZERO),
            Rc::new(options),
        );
        *window.window().0.render_solid.borrow_mut() =
            Some(SolidColorBuffer::new((120., 80.), [0., 1., 0., 1.]));
        *window.window().0.render_solid_blocked.borrow_mut() =
            Some(SolidColorBuffer::new((120., 80.), [0., 0., 0., 1.]));
        let size = window.tile_size();
        window.set_shrink_preview((40., 80.).into(), false);
        window.update_render_elements(true, Rectangle::from_size((1280., 720.).into()));
        assert_eq!(window.tile_size(), size);
        assert_eq!(window.form(), WindowForm::Normal);
        assert_eq!(window.window().requested_size(), None);
        let collect = |window: &Tile<TestWindow>,
                       renderer: &mut smithay::backend::renderer::gles::GlesRenderer,
                       target| {
            let mut elements = Vec::new();
            window.render(
                RenderCtx {
                    renderer,
                    target,
                    xray: None,
                },
                (0., 0.).into(),
                XrayPos::default(),
                false,
                &mut |e| elements.push(e),
            );
            elements
        };
        // Collect both targets before drawing: a blocked cache must not overwrite output pixels.
        let output = collect(&window, &mut renderer, RenderTarget::Output);
        let revisions = output
            .iter()
            .map(|e| (e.id().clone(), e.current_commit()))
            .collect::<Vec<_>>();
        let output_id = window
            .window()
            .0
            .offscreen_data
            .borrow()
            .as_ref()
            .unwrap()
            .id
            .clone();
        assert_eq!(output_id, revisions.last().unwrap().0);
        let blocked = collect(&window, &mut renderer, RenderTarget::ScreenCapture);
        assert_eq!(
            window
                .window()
                .0
                .offscreen_data
                .borrow()
                .as_ref()
                .unwrap()
                .id,
            output_id
        );
        let screencast = collect(&window, &mut renderer, RenderTarget::Screencast);
        assert_eq!(
            window
                .window()
                .0
                .offscreen_data
                .borrow()
                .as_ref()
                .unwrap()
                .id,
            output_id
        );
        drop(screencast);
        let physical = size.to_physical_precise_round(scale);
        let download = render_and_download(
            &mut renderer,
            physical,
            Scale::from(scale),
            Transform::Normal,
            Fourcc::Abgr8888,
            output.into_iter().rev(),
        )
        .unwrap();
        let data = renderer.map_texture(&download).unwrap();
        let at = |x: usize, y: usize| &data[(y * physical.w as usize + x) * 4..][..4];
        // The pure ghost uses the native linear-sRGB gradient midpoint, not its first stop.
        for point in [(5., 5.), (30., 50.)] {
            let p = at(
                (point.0 * scale).round() as usize,
                (point.1 * scale).round() as usize,
            );
            assert!(
                p[0].abs_diff(186) <= 1 && p[1] == 0 && p[2].abs_diff(186) <= 1 && p[3] == 255,
                "{p:?}"
            );
        }
        let gray = at(
            (60. * scale).round() as usize,
            (20. * scale).round() as usize,
        );
        assert!(
            gray[0].abs_diff(182) <= 1
                && gray[0] == gray[1]
                && gray[1] == gray[2]
                && gray[3] == 255,
            "{gray:?}"
        );
        let download = render_and_download(
            &mut renderer,
            physical,
            Scale::from(scale),
            Transform::Normal,
            Fourcc::Abgr8888,
            blocked.into_iter().rev(),
        )
        .unwrap();
        assert!(renderer
            .map_texture(&download)
            .unwrap()
            .chunks_exact(4)
            .all(|p| p == [0, 0, 0, 255]));
        let next = collect(&window, &mut renderer, RenderTarget::Output);
        assert_eq!(
            revisions,
            next.iter()
                .map(|e| (e.id().clone(), e.current_commit()))
                .collect::<Vec<_>>()
        );
        // Left-edge spatial resize retains the right edge of the normal source.
        window.set_shrink_preview_anchor((1., 0.).into());
        let anchored = collect(&window, &mut renderer, RenderTarget::Output);
        let download = render_and_download(
            &mut renderer,
            physical,
            Scale::from(scale),
            Transform::Normal,
            Fourcc::Abgr8888,
            anchored.into_iter().rev(),
        )
        .unwrap();
        let data = renderer.map_texture(&download).unwrap();
        let pixel = |x: f64| {
            let x = (x * scale).round() as usize;
            let y = (5. * scale).round() as usize;
            &data[(y * physical.w as usize + x) * 4..][..4]
        };
        assert!(pixel(5.)[0].abs_diff(182) <= 1 && pixel(5.)[0] == pixel(5.)[1]);
        assert!(
            pixel(100.)[0].abs_diff(186) <= 1
                && pixel(100.)[1] == 0
                && pixel(100.)[2].abs_diff(186) <= 1
        );
        window.clear_shrink_preview();
        assert!(window.shrink_preview_pose().is_none());
        let elements = collect(&window, &mut renderer, RenderTarget::Output);
        assert!(window.window().0.offscreen_data.borrow().is_none());
        let download = render_and_download(
            &mut renderer,
            physical,
            Scale::from(scale),
            Transform::Normal,
            Fourcc::Abgr8888,
            elements.into_iter().rev(),
        )
        .unwrap();
        assert!(renderer
            .map_texture(&download)
            .unwrap()
            .chunks_exact(4)
            .all(|p| p == [0, 255, 0, 255]));
    }
}

#[test]
#[ignore = "requires surfaceless EGL and the isolated desktop/icon fixture environment"]
fn spine_application_icon_stays_upright_and_private() {
    use crate::render_helpers::{render_and_download, RenderTarget};
    use smithay::backend::allocator::Fourcc;
    use smithay::backend::renderer::element::Element as _;
    use smithay::backend::renderer::ExportMem as _;
    let app_id =
        std::env::var("NIRI_SPINE_ICON_APP_ID").expect("supply isolated desktop fixture app ID");
    let mut renderer = spine_test_renderer();
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        for scale in [1., 1.25] {
            let mut options = Options::default();
            options.layout.border.off = true;
            options.layout.focus_ring.off = true;
            let mut params = TestWindowParams::new(1);
            params.rules = Some(ResolvedWindowRules {
                block_out_from: Some(niri_config::BlockOutFrom::ScreenCapture),
                ..Default::default()
            });
            let mut book = Tile::new(
                TestWindow::new(params),
                (1280., 720.).into(),
                scale,
                Clock::with_time(Duration::ZERO),
                Rc::new(options),
            );
            *book.window().0.spine_app_id.borrow_mut() = app_id.clone();
            assert!(book.set_spine(axis, 40.));
            book.request_tile_size((400., 280.).into(), false, None);
            book.update_render_elements(false, Rectangle::from_size((1280., 720.).into()));
            let mut collect = |target| {
                let mut elements = Vec::new();
                book.render(
                    RenderCtx {
                        renderer: &mut renderer,
                        target,
                        xray: None,
                    },
                    (0., 0.).into(),
                    XrayPos::default(),
                    false,
                    &mut |e| elements.push(e),
                );
                elements
            };
            let elements = collect(RenderTarget::Output);
            let revisions = elements
                .iter()
                .map(|e| (e.id().clone(), e.current_commit()))
                .collect::<Vec<_>>();
            assert_eq!(
                revisions,
                collect(RenderTarget::Output)
                    .iter()
                    .map(|e| (e.id().clone(), e.current_commit()))
                    .collect::<Vec<_>>()
            );
            let physical = book.tile_size().to_physical_precise_round(scale);
            let download = render_and_download(
                &mut renderer,
                physical,
                Scale::from(scale),
                Transform::Normal,
                Fourcc::Abgr8888,
                elements.into_iter().rev(),
            )
            .unwrap();
            let data = renderer.map_texture(&download).unwrap();
            let (x, y) = match axis {
                ShrinkAxis::Horizontal => (
                    (12. * scale).round() as usize,
                    (4. * scale).round() as usize,
                ),
                ShrinkAxis::Vertical => (
                    (4. * scale).round() as usize,
                    (12. * scale).round() as usize,
                ),
            };
            let extent = (16. * scale).round() as usize;
            let at = |x: usize, y: usize| &data[(y * physical.w as usize + x) * 4..][..4];
            assert_eq!(at(x + 2, y + 2), &[255, 0, 0, 255]);
            assert_eq!(at(x + extent - 3, y + 2), &[0, 255, 0, 255]);
            assert_eq!(at(x + 2, y + extent - 3), &[0, 0, 255, 255]);
            assert_eq!(at(x + extent - 3, y + extent - 3), &[255, 255, 0, 255]);
            let mut blocked = Vec::new();
            book.render(
                RenderCtx {
                    renderer: &mut renderer,
                    target: RenderTarget::ScreenCapture,
                    xray: None,
                },
                (0., 0.).into(),
                XrayPos::default(),
                false,
                &mut |e| blocked.push(e),
            );
            let download = render_and_download(
                &mut renderer,
                physical,
                Scale::from(scale),
                Transform::Normal,
                Fourcc::Abgr8888,
                blocked.into_iter().rev(),
            )
            .unwrap();
            assert!(renderer
                .map_texture(&download)
                .unwrap()
                .chunks_exact(4)
                .all(|p| p == [0, 0, 0, 255]));
            assert_eq!(book.window().0.render_normal_calls.get(), 0);
        }
    }
}

#[test]
#[ignore = "requires an explicit surfaceless EGL renderer run"]
fn spine_rendering_is_independent_private_and_stable_at_fractional_scale() {
    use smithay::backend::allocator::Fourcc;
    use smithay::backend::renderer::element::Element as _;
    use smithay::backend::renderer::gles::GlesRenderer;
    use smithay::backend::renderer::ExportMem as _;

    use crate::render_helpers::{render_and_download, RenderTarget};
    let mut renderer = spine_test_renderer();
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        for scale in [1., 1.25] {
            let mut options = Options::default();
            options.layout.focus_ring.off = true;
            options.layout.border.off = true;
            options.layout.spine.inactive =
                niri_config::SpinePaint::Solid("#204060".parse().unwrap());
            options.layout.spine.active =
                niri_config::SpinePaint::Solid("#ffa020".parse().unwrap());
            let mut params = TestWindowParams::new(1);
            params.rules = Some(ResolvedWindowRules {
                block_out_from: Some(niri_config::BlockOutFrom::ScreenCapture),
                ..Default::default()
            });
            let mut book = Tile::new(
                TestWindow::new(params),
                (1280., 720.).into(),
                scale,
                Clock::with_time(Duration::ZERO),
                Rc::new(options),
            );
            assert!(book.set_spine(axis, 40.));
            book.request_tile_size((400., 280.).into(), false, None);
            let size = book.tile_size();
            book.update_render_elements(true, Rectangle::from_size((1280., 720.).into()));
            let collect = |book: &Tile<TestWindow>, renderer: &mut GlesRenderer, target| {
                let mut elements = Vec::new();
                book.render(
                    RenderCtx {
                        renderer,
                        target,
                        xray: None,
                    },
                    (0., 0.).into(),
                    XrayPos::default(),
                    false,
                    &mut |elem| elements.push(elem),
                );
                elements
            };
            let first = collect(&book, &mut renderer, RenderTarget::Output);
            assert!(first
                .iter()
                .any(|e| matches!(e, crate::layout::tile::TileRenderElement::Spine(_))));
            let revisions = first
                .iter()
                .map(|e| (e.id().clone(), e.current_commit()))
                .collect::<Vec<_>>();
            book.update_render_elements(true, Rectangle::from_size((1280., 720.).into()));
            let second = collect(&book, &mut renderer, RenderTarget::Output);
            assert_eq!(
                revisions,
                second
                    .iter()
                    .map(|e| (e.id().clone(), e.current_commit()))
                    .collect::<Vec<_>>()
            );
            assert_eq!(book.window().0.render_normal_calls.get(), 0);
            assert_eq!(book.window().0.render_popup_calls.get(), 0);
            assert_eq!(book.window().0.render_background_calls.get(), 0);
            let physical = size.to_physical_precise_round(scale);
            let download = render_and_download(
                &mut renderer,
                physical,
                Scale::from(scale),
                Transform::Normal,
                Fourcc::Abgr8888,
                first.into_iter().rev(),
            )
            .unwrap();
            let data = renderer.map_texture(&download).unwrap().to_vec();
            let at = |x: usize, y: usize| &data[(y * physical.w as usize + x) * 4..][..4];
            assert_eq!(at(2, 2), &[32, 64, 96, 255]);
            if let Ok(directory) = std::env::var("NIRI_SPINE_EVIDENCE_DIR") {
                let path =
                    std::path::Path::new(&directory).join(format!("spine-{axis:?}-{scale}.png"));
                let file = std::fs::File::create(path).unwrap();
                let mut encoder = png::Encoder::new(file, physical.w as u32, physical.h as u32);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder
                    .write_header()
                    .unwrap()
                    .write_image_data(&data)
                    .unwrap();
            }
            // Sensitive title/icon pixels must not reappear from the unblocked texture cache.
            for target in [RenderTarget::Screencast, RenderTarget::ScreenCapture] {
                let elements = collect(&book, &mut renderer, target);
                let download = render_and_download(
                    &mut renderer,
                    physical,
                    Scale::from(scale),
                    Transform::Normal,
                    Fourcc::Abgr8888,
                    elements.into_iter().rev(),
                )
                .unwrap();
                let data = renderer.map_texture(&download).unwrap();
                assert!(data.chunks_exact(4).all(|pixel| pixel == [0, 0, 0, 255]));
            }
            // A changed caption damages its own texture once, without waking client rendering.
            *book.window().0.spine_title.borrow_mut() = "Updated title 更新".into();
            let changed = collect(&book, &mut renderer, RenderTarget::Output);
            assert_ne!(
                revisions,
                changed
                    .iter()
                    .map(|e| (e.id().clone(), e.current_commit()))
                    .collect::<Vec<_>>()
            );
            let changed_revisions = changed
                .iter()
                .map(|e| (e.id().clone(), e.current_commit()))
                .collect::<Vec<_>>();
            assert_eq!(
                changed_revisions,
                collect(&book, &mut renderer, RenderTarget::Output)
                    .iter()
                    .map(|e| (e.id().clone(), e.current_commit()))
                    .collect::<Vec<_>>()
            );
            let check_fill =
                |book: &Tile<TestWindow>, renderer: &mut GlesRenderer, expected: [u8; 4]| {
                    let elements = collect(book, renderer, RenderTarget::Output);
                    let download = render_and_download(
                        renderer,
                        physical,
                        Scale::from(scale),
                        Transform::Normal,
                        Fourcc::Abgr8888,
                        elements.into_iter().rev(),
                    )
                    .unwrap();
                    let data = renderer.map_texture(&download).unwrap();
                    let pixel = &data[(2 * physical.w as usize + 2) * 4..][..4];
                    // RGBA8 conversion of half alpha may round to 127 or 128.
                    assert!(
                        pixel
                            .iter()
                            .zip(expected)
                            .all(|(actual, expected)| actual.abs_diff(expected) <= 1),
                        "fill {pixel:?}, expected {expected:?}"
                    );
                };
            book.set_spine_visual_active(true);
            book.update_render_elements(false, Rectangle::from_size((1280., 720.).into()));
            check_fill(&book, &mut renderer, [255, 160, 32, 255]);
            let mut options = (*book.options).clone();
            options.layout.spine.urgent =
                niri_config::SpinePaint::Solid("#802040".parse().unwrap());
            book.update_config((1280., 720.).into(), scale, Rc::new(options));
            book.window().0.urgent.set(true);
            book.update_render_elements(false, Rectangle::from_size((1280., 720.).into()));
            check_fill(&book, &mut renderer, [128, 32, 64, 255]);
            book.window().0.urgent.set(false);
            book.set_spine_visual_active(false);
            let mut options = (*book.options).clone();
            options.layout.spine.inactive = niri_config::SpinePaint::Inherit;
            options.layout.border.inactive_color = "#308050".parse().unwrap();
            book.update_config((1280., 720.).into(), scale, Rc::new(options));
            book.update_render_elements(true, Rectangle::from_size((1280., 720.).into()));
            check_fill(&book, &mut renderer, [48, 128, 80, 255]);
            Rc::get_mut(&mut book.window_mut().0).unwrap().rules.opacity = Some(0.5);
            check_fill(&book, &mut renderer, [24, 64, 40, 128]);
            let faded = collect(&book, &mut renderer, RenderTarget::Output)
                .iter()
                .map(|e| (e.id().clone(), e.current_commit()))
                .collect::<Vec<_>>();
            assert_eq!(
                faded,
                collect(&book, &mut renderer, RenderTarget::Output)
                    .iter()
                    .map(|e| (e.id().clone(), e.current_commit()))
                    .collect::<Vec<_>>()
            );
            Rc::get_mut(&mut book.window_mut().0).unwrap().rules.opacity = None;
            let mut options = (*book.options).clone();
            options.layout.spine.inactive =
                niri_config::SpinePaint::Gradient(niri_config::Gradient {
                    from: "#ff0000".parse().unwrap(),
                    to: "#0000ff".parse().unwrap(),
                    angle: 90,
                    relative_to: niri_config::GradientRelativeTo::WorkspaceView,
                    in_: Default::default(),
                });
            book.update_config((1280., 720.).into(), scale, Rc::new(options));
            book.update_render_elements(
                false,
                Rectangle::new((-400., 0.).into(), (800., 720.).into()),
            );
            let elements = collect(&book, &mut renderer, RenderTarget::Output);
            let download = render_and_download(
                &mut renderer,
                physical,
                Scale::from(scale),
                Transform::Normal,
                Fourcc::Abgr8888,
                elements.into_iter().rev(),
            )
            .unwrap();
            let data = renderer.map_texture(&download).unwrap();
            let left = &data[(2 * physical.w as usize + 2) * 4..][..4];
            let right = &data[(2 * physical.w as usize + physical.w as usize - 3) * 4..][..4];
            assert!(left[0] > right[0] && left[2] < right[2]);
            assert!(
                left[0] > 0 && left[2] > 0,
                "workspace-relative fill must start midway through its gradient"
            );
            assert_eq!(left[3], 255);
            // At the unlocked-axis minimum, only the centred icon remains.
            book.request_tile_size((40., 40.).into(), false, None);
            book.update_render_elements(false, Rectangle::from_size((1280., 720.).into()));
            let short = collect(&book, &mut renderer, RenderTarget::Output);
            let caption_count = |elements: &Vec<_>| {
                elements
                    .iter()
                    .filter(|e| {
                        matches!(
                            e,
                            crate::layout::tile::TileRenderElement::Spine(
                                crate::ui::spine::SpineRenderElement::Caption(_)
                            )
                        )
                    })
                    .count()
            };
            assert_eq!(caption_count(&short), 1);
            // A large live border still leaves one physical pixel of independent fill.
            let rules = &mut Rc::get_mut(&mut book.window_mut().0).unwrap().rules;
            rules.border.on = true;
            rules.border.width = Some(niri_config::FloatOrInt(30.));
            book.update_window();
            book.update_render_elements(false, Rectangle::from_size((1280., 720.).into()));
            let tiny = collect(&book, &mut renderer, RenderTarget::Output);
            assert_eq!(caption_count(&tiny), 0);
            assert!(tiny.iter().any(|e| matches!(
                e,
                crate::layout::tile::TileRenderElement::Spine(
                    crate::ui::spine::SpineRenderElement::Fill(_)
                )
            )));
            assert_eq!(book.window().requested_size(), None);
            assert_eq!(book.window().0.render_normal_calls.get(), 0);
            assert!(book.clear_spine());
            let _ = collect(&book, &mut renderer, RenderTarget::Output);
            assert!(book.window().0.render_normal_calls.get() > 0);
            assert!(book.window().0.render_popup_calls.get() > 0);
            assert!(book.window().0.render_background_calls.get() > 0);
        }
    }
}

#[test]
fn floating_spine_resize_keeps_native_top_left_anchor_without_client_commit() {
    let mut tile = tile(1, 1., Rc::default());
    assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
    let mut space = FloatingSpace::new(
        (1280., 720.).into(),
        Rectangle::from_size((1280., 720.).into()),
        1.,
        tile.clock.clone(),
        Rc::default(),
    );
    space.add_tile(tile, true);
    let (_, before) = space.tiles_with_offsets().next().unwrap();
    let before_size = space.tiles().next().unwrap().tile_size();
    assert!(space.interactive_resize_begin(1, ResizeEdge::TOP_LEFT));
    assert!(space.interactive_resize_update(&1, (20., 100.).into()));
    let (tile, after) = space.tiles_with_offsets().next().unwrap();
    assert_eq!(tile.tile_size(), Size::from((40., before_size.h - 100.)));
    approx::assert_abs_diff_eq!(after.x, before.x, epsilon = 1e-9);
    approx::assert_abs_diff_eq!(after.y, before.y + 100., epsilon = 1e-9);
    assert_eq!(tile.window().requested_size(), None);
    space.verify_invariants();
    space.interactive_resize_end(Some(&1));
    space.verify_invariants();
}

#[test]
fn spine_width_preset_search_uses_outer_width_instead_of_hidden_client_width() {
    let mut options = Options::default();
    options.layout.preset_column_widths = vec![
        PresetSize::Fixed(300),
        PresetSize::Fixed(500),
        PresetSize::Fixed(700),
    ];
    let options = Rc::new(options);
    let mut space = scrolling(1., options.clone());
    let mut tile = scrolling_tile(1, &space, options);
    assert!(tile.set_spine(ShrinkAxis::Vertical, 40.));
    space.add_tile(None, tile, true, ColumnWidth::Fixed(400.), false, None);
    space.toggle_window_width(Some(&1), true);
    assert_eq!(space.tiles().next().unwrap().tile_size().w, 500.);
    assert_eq!(
        space.tiles().next().unwrap().window().requested_size(),
        None
    );
    space.verify_invariants();
}

#[test]
fn scrolling_spine_resize_uses_outer_extent_and_preserves_native_top_edge_rule() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let options: Rc<Options> = Rc::default();
        let mut space = scrolling(1., options.clone());
        let mut tile = scrolling_tile(1, &space, options);
        assert!(tile.set_spine(axis, 40.));
        space.add_tile(None, tile, true, ColumnWidth::Fixed(400.), false, None);
        let before = space.tiles().next().unwrap().tile_size();
        assert!(space.interactive_resize_begin(1, ResizeEdge::TOP));
        assert!(space.interactive_resize_update(&1, (0., 100.).into()));
        assert_eq!(space.tiles().next().unwrap().tile_size(), before);
        space.interactive_resize_end(Some(&1));
        assert!(space.interactive_resize_begin(1, ResizeEdge::BOTTOM_RIGHT));
        assert!(space.interactive_resize_update(&1, (50., -100.).into()));
        let tile = space.tiles().next().unwrap();
        let expected = match axis {
            ShrinkAxis::Horizontal => (40., before.h - 100.),
            ShrinkAxis::Vertical => (before.w + 50., 40.),
        };
        assert_eq!(tile.tile_size(), Size::from(expected));
        assert_eq!(tile.window().requested_size(), None);
        space.verify_invariants();
        space.interactive_resize_end(Some(&1));
    }
}

#[test]
fn ordinary_space_switch_finds_normal_receiver_and_skips_all_spine_space() {
    for target_floating in [false, true] {
        for normal_receiver in [false, true] {
            let mut workspace =
                Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::default());
            // A target may contain real windows but no client focus receiver.
            for (id, spine, floating) in [
                (1, false, !target_floating),
                (2, !normal_receiver, target_floating),
                (3, true, target_floating),
            ] {
                let mut tile = workspace.make_tile(TestWindow::new(TestWindowParams::new(id)));
                if spine {
                    assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
                }
                workspace.add_tile(
                    tile,
                    WorkspaceAddWindowTarget::Auto,
                    ActivateWindow::Yes,
                    ColumnWidth::Fixed(400.),
                    false,
                    floating,
                    None,
                );
            }
            workspace.activate_window(&1);
            workspace.switch_focus_floating_tiling();
            let expected = if normal_receiver { 2 } else { 1 };
            assert_eq!(workspace.active_window().unwrap().id(), &expected);
            assert_eq!(workspace.windows().count(), 3);
            assert_eq!(
                workspace.tiles().filter(|t| t.is_spine()).count(),
                if normal_receiver { 1 } else { 2 }
            );
            // An explicit request has the same ordinary focus semantics.
            workspace.activate_window(&1);
            if target_floating {
                workspace.focus_floating();
            } else {
                workspace.focus_tiling();
            }
            assert_eq!(workspace.active_window().unwrap().id(), &expected);
            workspace.refresh(true, true);
            for tile in workspace.tiles().filter(|tile| tile.is_spine()) {
                assert!(!tile.window().0.pending_activated.get());
            }
            workspace.verify_invariants(None);
        }
    }
}

#[test]
fn axis_plans_are_read_only_and_match_committed_native_column_sizes() {
    for scale in [1., 1.25] {
        for display in [ColumnDisplay::Normal, ColumnDisplay::Tabbed] {
            for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
                let mut options = Options::default();
                options.layout.default_column_display = display;
                let options = Rc::new(options);
                let mut space = scrolling(scale, options.clone());
                let tile = scrolling_tile(1, &space, options.clone());
                let client = tile.window().clone();
                space.add_tile(None, tile, true, ColumnWidth::Fixed(800.), false, None);
                let other = scrolling_tile(2, &space, options);
                let other_client = other.window().clone();
                space.add_tile_to_column(0, None, other, false);
                settle(&mut space);
                let before = [client.requested_size(), other_client.requested_size()];
                let sizes = space.tiles().map(|t| t.tile_size()).collect::<Vec<_>>();
                let plans = space.spine_plans(&1, 40.1).unwrap();
                assert_eq!(
                    [client.requested_size(), other_client.requested_size()],
                    before
                );
                assert_eq!(
                    space.tiles().map(|t| t.tile_size()).collect::<Vec<_>>(),
                    sizes
                );
                assert!(space.tiles().all(|t| !t.is_spine()));
                assert_eq!(
                    plans.first().unwrap().geometry.axis,
                    if display == ColumnDisplay::Normal {
                        ShrinkAxis::Vertical
                    } else {
                        ShrinkAxis::Horizontal
                    }
                );
                assert!(plans.get(axis).fits);
                assert_eq!(plans.next(axis).unwrap().geometry.axis, axis.other());
                assert_eq!(plans.for_move(axis).geometry.axis, axis);
                space
                    .tiles_mut()
                    .find(|t| t.window().id() == &1)
                    .unwrap()
                    .set_spine(axis, 40.1);
                space.update_window(&1, None);
                assert_eq!(
                    space
                        .tiles()
                        .find(|t| t.window().id() == &1)
                        .unwrap()
                        .tile_size(),
                    plans.get(axis).geometry.outer_size()
                );
                settle(&mut space);
            }
        }
    }
}

#[test]
fn insertion_plans_match_native_new_column_and_in_column_without_reselecting_topology() {
    for scale in [1., 1.25] {
        for display in [ColumnDisplay::Normal, ColumnDisplay::Tabbed] {
            for in_column in [false, true] {
                for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
                    let mut options = Options::default();
                    options.layout.default_column_display = display;
                    let options = Rc::new(options);
                    let mut space = scrolling(scale, options.clone());
                    let first = scrolling_tile(1, &space, options.clone());
                    space.add_tile(None, first, true, ColumnWidth::Fixed(800.), false, None);
                    settle(&mut space);
                    let mut incoming = scrolling_tile(2, &space, options);
                    let client = incoming.window().clone();
                    let plans = if in_column {
                        space.spine_in_column_plans(0, 1, &incoming, 40.1)
                    } else {
                        space.spine_new_column_plans(
                            &incoming,
                            ColumnWidth::Fixed(700.),
                            false,
                            40.1,
                        )
                    }
                    .unwrap();
                    assert_eq!(client.requested_size(), None);
                    assert_eq!(space.tiles().count(), 1);
                    assert_eq!(space.columns().count(), 1);
                    assert!(incoming.set_spine(axis, 40.1));
                    if in_column {
                        space.add_tile_to_column(0, Some(1), incoming, true);
                    } else {
                        space.add_tile(
                            Some(1),
                            incoming,
                            true,
                            ColumnWidth::Fixed(700.),
                            false,
                            None,
                        );
                    }
                    assert_eq!(space.columns().count(), if in_column { 1 } else { 2 });
                    assert_eq!(
                        space
                            .tiles()
                            .find(|t| t.window().id() == &2)
                            .unwrap()
                            .tile_size(),
                        plans.get(axis).geometry.outer_size()
                    );
                    assert_eq!(client.requested_size(), None);
                    settle(&mut space);
                }
            }
        }
    }
}

#[test]
fn axis_legality_precedes_local_preference_when_native_neighbor_is_oversized() {
    let options: Rc<Options> = Rc::default();
    let mut space = ScrollingSpace::new(
        (320., 720.).into(),
        Rectangle::from_size((320., 720.).into()),
        1.,
        Clock::with_time(Duration::ZERO),
        options.clone(),
    );
    let tile = scrolling_tile(1, &space, options.clone());
    space.add_tile(None, tile, true, ColumnWidth::Fixed(800.), false, None);
    let mut incoming = scrolling_tile(2, &space, options);
    let plans = space.spine_in_column_plans(0, 1, &incoming, 40.).unwrap();
    assert!(plans.get(ShrinkAxis::Horizontal).fits);
    assert!(!plans.get(ShrinkAxis::Vertical).fits);
    assert_eq!(plans.first().unwrap().geometry.axis, ShrinkAxis::Horizontal);
    assert_eq!(
        plans.next(ShrinkAxis::Horizontal).unwrap().geometry.axis,
        ShrinkAxis::Horizontal
    );
    let chosen = plans.for_move(ShrinkAxis::Vertical);
    assert_eq!(chosen.geometry.axis, ShrinkAxis::Horizontal);
    assert!(incoming.set_spine(chosen.geometry.axis, 40.));
    space.add_tile_to_column(0, Some(1), incoming, true);
    assert_eq!(space.columns().count(), 1);
    assert_eq!(
        space
            .tiles()
            .find(|t| t.window().id() == &2)
            .unwrap()
            .tile_size(),
        chosen.geometry.outer_size()
    );
    settle(&mut space);
}

#[test]
fn both_axes_too_small_keep_move_axis_and_use_native_overflow_allocation() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let options: Rc<Options> = Rc::default();
        let mut space = ScrollingSpace::new(
            (20., 20.).into(),
            Rectangle::from_size((20., 20.).into()),
            1.,
            Clock::with_time(Duration::ZERO),
            options.clone(),
        );
        let mut incoming = scrolling_tile(1, &space, options);
        let plans = space
            .spine_new_column_plans(&incoming, ColumnWidth::Fixed(400.), false, 40.)
            .unwrap();
        assert!(plans.first().is_none());
        assert!(plans.next(axis).is_none());
        let chosen = plans.for_move(axis);
        assert_eq!(chosen.geometry.axis, axis);
        assert!(!chosen.fits);
        assert!(incoming.set_spine(axis, 40.));
        space.add_tile(None, incoming, true, ColumnWidth::Fixed(400.), false, None);
        assert_eq!(
            space.tiles().next().unwrap().tile_size(),
            chosen.geometry.outer_size()
        );
        assert_eq!(space.columns().count(), 1);
        settle(&mut space);
    }
}

#[test]
fn sibling_minimum_heights_can_make_both_axes_illegal_without_changing_native_insertion() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let options: Rc<Options> = Rc::default();
        let mut space = scrolling(1., options.clone());
        let first = scrolling_tile(1, &space, options.clone());
        space.add_tile(None, first, true, ColumnWidth::Fixed(800.), false, None);
        let second = scrolling_tile(2, &space, options.clone());
        space.add_tile_to_column(0, None, second, false);
        settle(&mut space);
        let mut incoming = scrolling_tile(3, &space, options);
        let plans = space.spine_in_column_plans(0, 2, &incoming, 40.).unwrap();
        assert!(!plans.get(ShrinkAxis::Horizontal).fits);
        assert!(!plans.get(ShrinkAxis::Vertical).fits);
        assert!(plans.first().is_none());
        let chosen = plans.for_move(axis);
        assert_eq!(chosen.geometry.axis, axis);
        assert!(incoming.set_spine(axis, 40.));
        space.add_tile_to_column(0, Some(2), incoming, true);
        assert_eq!(space.columns().count(), 1);
        assert_eq!(space.tiles().count(), 3);
        assert_eq!(
            space
                .tiles()
                .find(|t| t.window().id() == &3)
                .unwrap()
                .tile_size(),
            chosen.geometry.outer_size()
        );
        settle(&mut space);
    }
}

#[test]
fn floating_axis_plans_are_read_only_and_default_to_horizontal_shrink() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let options: Rc<Options> = Rc::default();
        let tile = tile(1, 1., options.clone());
        let client = tile.window().clone();
        let mut space = FloatingSpace::new(
            (1280., 720.).into(),
            Rectangle::from_size((1280., 720.).into()),
            1.,
            tile.clock.clone(),
            options,
        );
        space.add_tile(tile, true);
        let before = client.requested_size();
        let plans = space.spine_plans(&1, 40.).unwrap();
        assert_eq!(plans.first().unwrap().geometry.axis, ShrinkAxis::Horizontal);
        assert_eq!(client.requested_size(), before);
        assert!(!space.tiles().next().unwrap().is_spine());
        let tile = space.tiles_mut().next().unwrap();
        assert!(tile.set_spine(axis, 40.));
        space.update_window(&1, None);
        assert_eq!(
            space.tiles().next().unwrap().tile_size(),
            plans.get(axis).geometry.outer_size()
        );
        assert_eq!(client.requested_size(), before);
        space.verify_invariants();
    }
}

#[test]
fn initial_shrink_query_rejects_transients_sink_spines_and_non_normal_modes_without_mutation() {
    let mut layout = Layout::<TestWindow>::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    assert!(layout.shrink_plans(&1, 40.).unwrap().first().is_some());
    assert!(layout.shrink_plans(&99, 40.).is_none());
    assert!(layout.shrink_plans(&1, f64::NAN).is_none());
    let window = layout
        .windows()
        .find(|(_, w)| w.id() == &1)
        .unwrap()
        .1
        .clone();
    let before = window.requested_size();
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        window.0.pending_sizing_mode.set(mode);
        assert!(layout.shrink_plans(&1, 40.).is_none());
        assert_eq!(window.requested_size(), before);
    }
    window.0.pending_sizing_mode.set(SizingMode::Normal);
    let mut params = TestWindowParams::new(2);
    params.parent_id = Some(1);
    Op::AddWindow { params }.apply(&mut layout);
    assert!(layout.shrink_plans(&1, 40.).is_none());
    assert!(layout.shrink_plans(&2, 40.).is_none());
    let child = layout
        .windows()
        .find(|(_, w)| w.id() == &2)
        .unwrap()
        .1
        .clone();
    child.0.parent_id.set(Some(99));
    assert!(layout.shrink_plans(&2, 40.).is_none());
    child.0.parent_id.set(None);
    assert!(layout.shrink_plans(&1, 40.).is_some());
    let workspace = layout
        .workspaces_mut()
        .find(|ws| ws.has_window(&1))
        .unwrap();
    assert!(workspace.sink_window(&1));
    assert!(layout.shrink_plans(&1, 40.).is_none());
    let workspace = layout
        .workspaces_mut()
        .find(|ws| ws.has_window(&2))
        .unwrap();
    let tile = workspace
        .tiles_mut()
        .find(|t| t.window().id() == &2)
        .unwrap();
    assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
    assert!(layout.shrink_plans(&2, 40.).is_none());
    assert_eq!(layout.windows().count(), 2);
    layout.update_window(&2, None);
    layout.verify_invariants();
}

#[test]
fn normal_restore_submits_live_default_at_same_slot_and_preserves_absent_focus() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let mut options = Options::default();
        options.layout.default_column_width = Some(PresetSize::Fixed(760));
        let mut workspace =
            Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::new(options));
        let client = TestWindow::new(TestWindowParams::new(1));
        let mut book = workspace.make_tile(client.clone());
        book.floating_window_size = Some((999, 888).into());
        assert!(book.set_spine(axis, 40.));
        workspace.add_tile(
            book,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            ColumnWidth::Fixed(1100.),
            true,
            false,
            None,
        );
        assert!(workspace.active_window().is_none());
        let expected = workspace.normal_spine_plans(&[1])[0].1;
        assert_eq!(workspace.restore_normal_spines(&[99, 1, 1]), vec![1]);
        assert_eq!(client.requested_size(), Some(expected.client_size));
        assert_eq!(workspace.tiles().next().unwrap().form(), WindowForm::Normal);
        assert!(workspace.active_window().is_none());
        assert_eq!(workspace.windows().count(), 1);
        assert_eq!(workspace.scrolling().columns().count(), 1);
        assert_eq!(
            workspace.tiles().next().unwrap().floating_window_size,
            Some((999, 888).into())
        );
        assert!(workspace.restore_normal_spines(&[1]).is_empty());
        for _ in 0..4 {
            if client.communicate() {
                workspace.update_window(&1, None);
            }
        }
        workspace.verify_invariants(None);
        workspace.refresh(true, true);
        assert!(!client.0.pending_activated.get());
        assert!(workspace.active_window().is_none());
        workspace.activate_window(&1);
        workspace.refresh(true, true);
        assert_eq!(workspace.active_window().unwrap().id(), &1);
        assert!(client.0.pending_activated.get());
    }
}

#[test]
fn normal_restore_batch_reuses_shared_column_solver_and_keeps_existing_focus() {
    for display in [ColumnDisplay::Normal, ColumnDisplay::Tabbed] {
        let mut options = Options::default();
        options.layout.default_column_display = display;
        let mut workspace =
            Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::new(options));
        let mut clients = Vec::new();
        for id in 1..=3 {
            let mut params = TestWindowParams::new(id);
            params.min_max_size = ((100, 20).into(), (0, 0).into());
            let client = TestWindow::new(params);
            let mut tile = workspace.make_tile(client.clone());
            if id > 1 {
                assert!(tile.set_spine(ShrinkAxis::Vertical, 40.));
            }
            if id == 1 {
                workspace.add_tile(
                    tile,
                    WorkspaceAddWindowTarget::Auto,
                    ActivateWindow::Yes,
                    ColumnWidth::Fixed(700.),
                    false,
                    false,
                    None,
                );
            } else {
                workspace
                    .scrolling_mut()
                    .add_tile_to_column(0, None, tile, false);
            }
            clients.push(client);
        }
        workspace
            .scrolling_mut()
            .set_window_height(Some(&2), SizeChange::SetFixed(200));
        let plans = workspace.normal_spine_plans(&[3, 2]);
        let focus = *workspace.active_window().unwrap().id();
        assert_eq!(workspace.restore_normal_spines(&[3, 2, 1, 2]), vec![2, 3]);
        for (id, plan) in &plans {
            assert_eq!(clients[id - 1].requested_size(), Some(plan.client_size));
        }
        for _ in 0..4 {
            for client in &clients {
                if client.communicate() {
                    workspace.update_window(client.id(), None);
                }
            }
        }
        workspace.verify_invariants(None);
        assert_eq!(*workspace.active_window().unwrap().id(), focus);
        assert_eq!(workspace.scrolling().columns().count(), 1);
        assert_eq!(
            workspace
                .tiles()
                .map(|t| *t.window().id())
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }
}

#[test]
fn normal_restore_in_non_normal_container_keeps_origin_until_mode_commit() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let mut space = non_normal_space(1.25, mode, ColumnDisplay::Tabbed);
        let options = space.tiles().next().unwrap().options().clone();
        let mut book = scrolling_tile(2, &space, options);
        assert!(book.set_spine(ShrinkAxis::Vertical, 40.));
        space.add_tile_to_column(0, None, book, true);
        space.remove_tile(&1, Transaction::new());
        let requests = vec![(2, ColumnWidth::Fixed(760.))];
        let plans = space.normal_spine_plans(&requests);
        assert!(space
            .tiles_mut()
            .next()
            .unwrap()
            .prepare_normal_restore(plans[0].1));
        space.commit_normal_restore(&plans, &requests, Transaction::new(), true);
        space.verify_invariants();
        assert_eq!(space.tiles().next().unwrap().sizing_mode(), mode);
        assert_eq!(
            space.tiles().next().unwrap().window().pending_sizing_mode(),
            mode
        );
        settle(&mut space);
        assert_eq!(space.tiles().next().unwrap().window().sizing_mode(), mode);
        space.verify_invariants();
    }
}

#[test]
fn floating_restore_requests_normal_defaults_and_preserves_logical_position_and_order() {
    for explicit_size in [false, true] {
        let options = Rc::new(Options::default());
        let mut workspace = Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), options);
        let mut params = TestWindowParams::new(1);
        params.bbox = Rectangle::from_size((240, 160).into());
        if explicit_size {
            params.rules = Some(ResolvedWindowRules {
                default_width: Some(Some(PresetSize::Fixed(720))),
                default_height: Some(Some(PresetSize::Fixed(400))),
                ..Default::default()
            });
        }
        let client = TestWindow::new(params);
        let mut book = workspace.make_tile(client.clone());
        book.floating_window_size = Some((999, 888).into());
        book.floating_pos = Some((0.2, 0.3).into());
        assert!(book.set_spine(ShrinkAxis::Horizontal, 40.));
        workspace.add_tile(
            book,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            ColumnWidth::Fixed(900.),
            false,
            true,
            None,
        );
        let before = workspace
            .floating()
            .tiles_with_offsets()
            .map(|(t, p)| (*t.window().id(), p))
            .collect::<Vec<_>>();
        let plan = workspace.normal_spine_plans(&[1])[0].1;
        assert_eq!(workspace.restore_normal_spines(&[1]), vec![1]);
        assert_eq!(client.requested_size(), Some(plan.client_size));
        assert_eq!(
            plan.client_size,
            if explicit_size {
                (720, 400).into()
            } else {
                (0, 0).into()
            }
        );
        assert_eq!(
            workspace
                .floating()
                .tiles_with_offsets()
                .map(|(t, p)| (*t.window().id(), p))
                .collect::<Vec<_>>(),
            before
        );
        for _ in 0..4 {
            if client.communicate() {
                workspace.update_window(&1, None);
            }
        }
        workspace.verify_invariants(None);
        assert_eq!(
            workspace
                .floating()
                .tiles_with_offsets()
                .map(|(t, p)| (*t.window().id(), p))
                .collect::<Vec<_>>(),
            before
        );
        assert!(workspace.active_window().is_none());
    }
}

#[test]
fn normal_restore_mode_unset_before_first_commit_keeps_native_view_restore_record() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let mut space = non_normal_space(1.25, mode, ColumnDisplay::Tabbed);
        let options = space.tiles().next().unwrap().options().clone();
        let mut book = scrolling_tile(2, &space, options);
        assert!(book.set_spine(ShrinkAxis::Vertical, 40.));
        space.add_tile_to_column(0, None, book, true);
        space.remove_tile(&1, Transaction::new());
        let requests = vec![(2, ColumnWidth::Fixed(760.))];
        let plans = space.normal_spine_plans(&requests);
        assert!(space
            .tiles_mut()
            .next()
            .unwrap()
            .prepare_normal_restore(plans[0].1));
        space.commit_normal_restore(&plans, &requests, Transaction::new(), true);
        // The client is still Normal; the column origin came from the former spine-only mode.
        assert_eq!(
            space.tiles().next().unwrap().window().sizing_mode(),
            SizingMode::Normal
        );
        if mode.is_fullscreen() {
            space.set_fullscreen(&2, false);
        } else {
            space.set_maximized(&2, false);
        }
        space.verify_invariants();
        settle(&mut space);
        space.verify_invariants();
        assert_eq!(
            space.tiles().next().unwrap().window().sizing_mode(),
            SizingMode::Normal
        );
    }
}

fn explicit_mode_layout(display: ColumnDisplay) -> Layout<TestWindow> {
    let mut options = Options::default();
    options.layout.default_column_display = display;
    let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    Op::Communicate(1).apply(&mut layout);
    let ws = layout
        .workspaces_mut()
        .find(|ws| ws.has_window(&1))
        .unwrap();
    for (id, axis) in [(2, ShrinkAxis::Horizontal), (3, ShrinkAxis::Vertical)] {
        let mut tile = ws.make_tile(TestWindow::new(TestWindowParams::new(id)));
        assert!(tile.set_spine(axis, 40.));
        ws.scrolling_mut().add_tile_to_column(0, None, tile, false);
    }
    layout.verify_invariants();
    layout
}

#[test]
fn explicit_duplicate_column_mode_restores_all_tabs_before_native_early_return() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let mut layout = explicit_mode_layout(ColumnDisplay::Tabbed);
        if mode.is_fullscreen() {
            layout.set_fullscreen(&1, true);
        } else {
            layout.set_maximized(&1, true);
        }
        Op::Communicate(1).apply(&mut layout);
        assert!(layout.is_spine(&2) && layout.is_spine(&3));
        if mode.is_fullscreen() {
            layout.request_fullscreen(&1, true);
        } else {
            layout.request_maximized(&1, true);
        }
        for id in [2, 3] {
            assert!(!layout.is_spine(&id));
            let win = layout.windows().find(|(_, w)| w.id() == &id).unwrap().1;
            assert_eq!(win.pending_sizing_mode(), mode);
            assert_eq!(
                win.requested_size(),
                layout
                    .windows()
                    .find(|(_, w)| w.id() == &1)
                    .unwrap()
                    .1
                    .requested_size()
            );
            Op::Communicate(id).apply(&mut layout);
        }
        assert_eq!(layout.windows().count(), 3);
        assert_eq!(layout.focus().unwrap().id(), &1);
        layout.verify_invariants();
    }
}

#[test]
fn explicit_toggle_uses_book_container_flags_before_restore_including_latent_maximize() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let mut layout = explicit_mode_layout(ColumnDisplay::Tabbed);
        if mode.is_fullscreen() {
            layout.set_fullscreen(&1, true);
        } else {
            layout.set_maximized(&1, true);
        }
        Op::Communicate(1).apply(&mut layout);
        assert_eq!(
            layout
                .windows()
                .find(|(_, w)| w.id() == &2)
                .unwrap()
                .1
                .pending_sizing_mode(),
            SizingMode::Normal
        );
        if mode.is_fullscreen() {
            layout.request_toggle_fullscreen(&2);
        } else {
            layout.request_toggle_maximized(&2);
        }
        let ws = layout
            .workspaces()
            .find(|(_, _, ws)| ws.has_window(&1))
            .unwrap()
            .2;
        let col = ws.scrolling().columns().next().unwrap();
        assert!(!col.is_pending_fullscreen() && !col.is_pending_maximized());
        assert!(!layout.is_spine(&2) && !layout.is_spine(&3));
        for id in [1, 2, 3] {
            Op::Communicate(id).apply(&mut layout);
        }
        layout.verify_invariants();
    }
    let mut layout = explicit_mode_layout(ColumnDisplay::Tabbed);
    layout.set_maximized(&1, true);
    layout.set_fullscreen(&1, true);
    Op::Communicate(1).apply(&mut layout);
    layout.request_toggle_maximized(&2);
    let ws = layout
        .workspaces()
        .find(|(_, _, ws)| ws.has_window(&1))
        .unwrap()
        .2;
    let col = ws.scrolling().columns().next().unwrap();
    assert!(col.is_pending_fullscreen());
    assert!(!col.is_pending_maximized());
    for id in [1, 2, 3] {
        Op::Communicate(id).apply(&mut layout);
    }
    layout.verify_invariants();
}

#[test]
fn explicit_mode_on_normal_column_keeps_native_extraction_and_other_spines() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let mut layout = explicit_mode_layout(ColumnDisplay::Normal);
        let other = layout
            .windows()
            .find(|(_, w)| w.id() == &3)
            .unwrap()
            .1
            .clone();
        if mode.is_fullscreen() {
            layout.request_fullscreen(&2, true);
        } else {
            layout.request_maximized(&2, true);
        }
        assert!(!layout.is_spine(&2));
        assert!(layout.is_spine(&3));
        assert_eq!(other.requested_size(), None);
        let ws = layout
            .workspaces()
            .find(|(_, _, ws)| ws.has_window(&1))
            .unwrap()
            .2;
        assert_eq!(ws.scrolling().columns().count(), 2);
        assert_eq!(
            layout
                .windows()
                .find(|(_, w)| w.id() == &2)
                .unwrap()
                .1
                .pending_sizing_mode(),
            mode
        );
        for id in [1, 2] {
            Op::Communicate(id).apply(&mut layout);
        }
        layout.verify_invariants();
        assert_eq!(layout.windows().count(), 3);
    }
}

#[test]
fn explicit_windowed_fullscreen_restores_only_affected_scope_and_waits_for_flag_commit() {
    for fullscreen_container in [false, true] {
        let mut layout = explicit_mode_layout(ColumnDisplay::Tabbed);
        if fullscreen_container {
            layout.set_fullscreen(&1, true);
            Op::Communicate(1).apply(&mut layout);
        }
        layout.request_toggle_windowed_fullscreen(&2);
        assert!(!layout.is_spine(&2));
        assert_eq!(layout.is_spine(&3), !fullscreen_container);
        let ws = layout
            .workspaces()
            .find(|(_, _, ws)| ws.has_window(&1))
            .unwrap()
            .2;
        let tile = ws.tiles().find(|t| t.window().id() == &2).unwrap();
        assert!(tile.window().is_pending_windowed_fullscreen());
        assert!(!tile.window().is_windowed_fullscreen());
        assert!(tile.normal_restore_input_blocked());
        for id in [1, 2, 3] {
            Op::Communicate(id).apply(&mut layout);
        }
        layout.verify_invariants();
        layout.request_toggle_windowed_fullscreen(&2);
        Op::Communicate(2).apply(&mut layout);
        assert!(!layout
            .windows()
            .find(|(_, w)| w.id() == &2)
            .unwrap()
            .1
            .is_windowed_fullscreen());
        layout.verify_invariants();
    }
}

#[test]
fn explicit_floating_modes_restore_form_and_keep_native_return_to_floating() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let mut layout = Layout::<TestWindow>::default();
        Op::AddOutput(1).apply(&mut layout);
        let mut params = TestWindowParams::new(1);
        params.is_floating = true;
        Op::AddWindow { params }.apply(&mut layout);
        Op::Communicate(1).apply(&mut layout);
        let ws = layout
            .workspaces_mut()
            .find(|ws| ws.has_window(&1))
            .unwrap();
        assert!(ws.is_floating(&1));
        assert!(ws
            .tiles_mut()
            .next()
            .unwrap()
            .set_spine(ShrinkAxis::Horizontal, 40.));
        ws.update_window(&1, None);
        let win = layout.windows().next().unwrap().1.clone();
        let output_events = win.0.output_events.borrow().clone();
        if mode.is_fullscreen() {
            layout.request_fullscreen(&1, true);
        } else {
            layout.request_maximized(&1, true);
        }
        assert!(!layout.is_spine(&1));
        assert!(!layout
            .workspaces()
            .find(|(_, _, ws)| ws.has_window(&1))
            .unwrap()
            .2
            .is_floating(&1));
        assert_eq!(win.pending_sizing_mode(), mode);
        Op::Communicate(1).apply(&mut layout);
        layout.verify_invariants();
        if mode.is_fullscreen() {
            layout.request_fullscreen(&1, false);
        } else {
            layout.request_maximized(&1, false);
        }
        Op::Communicate(1).apply(&mut layout);
        layout.verify_invariants();
        assert!(layout
            .workspaces()
            .find(|(_, _, ws)| ws.has_window(&1))
            .unwrap()
            .2
            .is_floating(&1));
        assert_eq!(
            win.0.output_events.borrow().as_slice(),
            output_events.as_slice()
        );
        assert!(Rc::ptr_eq(&win.0, &layout.windows().next().unwrap().1 .0));
    }
}

#[test]
fn explicit_unfullscreen_of_windowed_client_keeps_other_book_tabs_collapsed() {
    let mut layout = explicit_mode_layout(ColumnDisplay::Tabbed);
    layout.request_toggle_windowed_fullscreen(&2);
    Op::Communicate(2).apply(&mut layout);
    assert!(layout.is_spine(&3));
    layout.request_fullscreen(&2, false);
    assert!(layout.is_spine(&3));
    layout.request_fullscreen(&2, false);
    assert!(layout.is_spine(&3));
    Op::Communicate(2).apply(&mut layout);
    let win = layout.windows().find(|(_, w)| w.id() == &2).unwrap().1;
    assert!(!win.is_windowed_fullscreen());
    assert!(!win.is_pending_windowed_fullscreen());
    layout.request_fullscreen(&2, false);
    layout.request_maximized(&2, false);
    assert!(layout.is_spine(&3));
    layout.verify_invariants();
}

#[test]
#[ignore = "requires an explicit software EGL selected-content preview rendering run"]
fn expand_preview_projects_only_selected_content_without_configure_and_isolates_capture() {
    use crate::layout::shrink_expand::NormalSizePlan;
    use crate::layout::workspace::WorkspaceRenderElement;
    use crate::render_helpers::solid_color::SolidColorBuffer;
    use crate::render_helpers::{render_and_download, RenderTarget};
    use smithay::backend::allocator::Fourcc;
    use smithay::backend::renderer::element::Element as _;
    use smithay::backend::renderer::ExportMem as _;
    let mut renderer = spine_test_renderer();
    for scale in [1., 1.25] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            for opacity in [1., 0.5] {
                let mut options = Options::default();
                options.layout.border.off = true;
                options.layout.focus_ring.off = true;
                options.animations.off = true;
                let mut params = TestWindowParams::new(1);
                params.bbox = Rectangle::from_size((120, 80).into());
                params.rules = Some(ResolvedWindowRules {
                    opacity: Some(opacity),
                    block_out_from: Some(niri_config::BlockOutFrom::ScreenCapture),
                    ..Default::default()
                });
                let window = TestWindow::new(params);
                *window.0.render_solid.borrow_mut() =
                    Some(SolidColorBuffer::new((120., 80.), [0., 1., 0., 1.]));
                *window.0.render_solid_blocked.borrow_mut() =
                    Some(SolidColorBuffer::new((120., 80.), [0., 0., 0., 1.]));
                let mut tile = Tile::new(
                    window.clone(),
                    (1280., 720.).into(),
                    scale,
                    Clock::with_time(Duration::ZERO),
                    Rc::new(options),
                );
                assert!(tile.set_spine(axis, 40.));
                let spine_size = tile.tile_size();
                let plan = NormalSizePlan {
                    allocation: (600., 200.).into(),
                    client_size: (600, 200).into(),
                    sizing_mode: SizingMode::Normal,
                };
                tile.set_expand_preview(plan, 0.5, true);
                tile.update_render_elements(false, Rectangle::from_size((1280., 720.).into()));
                assert_eq!(tile.expand_preview_size(), Some((300., 100.).into()));
                assert_eq!(tile.tile_size(), spine_size);
                assert_eq!(window.requested_size(), None);
                let collect = |tile: &Tile<TestWindow>, renderer: &mut _, target| {
                    let mut elements: Vec<WorkspaceRenderElement<_>> = Vec::new();
                    let projected = tile
                        .render_expand_preview(
                            RenderCtx {
                                renderer,
                                target,
                                xray: None,
                            },
                            (300., 200.).into(),
                            XrayPos::default(),
                            true,
                            &mut |elem| elements.push(elem.into()),
                        )
                        .unwrap();
                    let geometry = projected.geometry(Scale::from(scale));
                    let id = projected.id().clone();
                    elements.push(projected.into());
                    (elements, geometry, id)
                };
                let (output, geometry, output_id) =
                    collect(&tile, &mut renderer, RenderTarget::Output);
                let (blocked, capture_geo, capture_id) =
                    collect(&tile, &mut renderer, RenderTarget::ScreenCapture);
                let (screencast, _, screencast_id) =
                    collect(&tile, &mut renderer, RenderTarget::Screencast);
                assert_eq!(geometry, capture_geo);
                assert_ne!(output_id, capture_id);
                assert_ne!(output_id, screencast_id);
                // The old 120x80 client is uniformly fitted into the predicted 300x100 frame.
                assert_eq!(
                    geometry.size,
                    Size::from((150., 100.)).to_physical_precise_round(scale)
                );
                let center: Point<i32, smithay::utils::Physical> = Point::<f64, Logical>::from((
                    300. + spine_size.w / 2.,
                    200. + spine_size.h / 2.,
                ))
                .to_physical_precise_round(scale);
                let drawn_center = geometry.loc + geometry.size.to_point().downscale(2);
                assert!(
                    (drawn_center.x - center.x).abs() <= 1
                        && (drawn_center.y - center.y).abs() <= 1
                );
                let physical = Size::from((900., 700.)).to_physical_precise_round(scale);
                for (elements, green, target_id) in [
                    (output, true, output_id.clone()),
                    (blocked, false, capture_id),
                    (screencast, false, screencast_id),
                ] {
                    let download = render_and_download(
                        &mut renderer,
                        physical,
                        Scale::from(scale),
                        Transform::Normal,
                        Fourcc::Abgr8888,
                        elements.into_iter().rev(),
                    )
                    .unwrap();
                    let data = renderer.map_texture(&download).unwrap();
                    let at = (center.y as usize * physical.w as usize + center.x as usize) * 4;
                    let pixel = &data[at..at + 4];
                    assert_eq!(pixel[0], 0);
                    assert_eq!(pixel[2], 0);
                    assert!(
                        pixel[1].abs_diff(if green {
                            (255. * opacity).round() as u8
                        } else {
                            0
                        }) <= 1,
                        "{pixel:?}"
                    );
                    assert!(
                        pixel[3].abs_diff((255. * opacity).round() as u8) <= 1,
                        "{pixel:?}"
                    );
                    assert_eq!(
                        window.0.offscreen_data.borrow().as_ref().unwrap().id,
                        output_id
                    );
                    assert_eq!(green, target_id == output_id);
                    if !green {
                        // Collecting the book itself in a capture pass must preserve output feedback.
                        tile.render(
                            RenderCtx {
                                renderer: &mut renderer,
                                target: RenderTarget::ScreenCapture,
                                xray: None,
                            },
                            (300., 200.).into(),
                            XrayPos::default(),
                            false,
                            &mut |_| {},
                        );
                        assert_eq!(
                            window.0.offscreen_data.borrow().as_ref().unwrap().id,
                            output_id
                        );
                    }
                }
                window
                    .0
                    .render_solid
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .set_color([1., 0., 0., 1.]);
                let (live, _, _) = collect(&tile, &mut renderer, RenderTarget::Output);
                let download = render_and_download(
                    &mut renderer,
                    physical,
                    Scale::from(scale),
                    Transform::Normal,
                    Fourcc::Abgr8888,
                    live.into_iter().rev(),
                )
                .unwrap();
                let data = renderer.map_texture(&download).unwrap();
                let at = (center.y as usize * physical.w as usize + center.x as usize) * 4;
                assert!(data[at].abs_diff((255. * opacity).round() as u8) <= 1);
                assert_eq!(data[at + 1], 0);
                assert_eq!(window.0.render_normal_calls.get(), 4);
                assert_eq!(window.0.render_background_calls.get(), 4);
                assert_eq!(window.0.render_popup_calls.get(), 4);
                tile.clear_expand_preview();
                assert!(tile.expand_preview_size().is_none());
                let calls = window.0.render_normal_calls.get();
                assert!(tile
                    .render_expand_preview(
                        RenderCtx {
                            renderer: &mut renderer,
                            target: RenderTarget::Output,
                            xray: None
                        },
                        (0., 0.).into(),
                        XrayPos::default(),
                        true,
                        &mut |_| {}
                    )
                    .is_none());
                assert_eq!(window.0.render_normal_calls.get(), calls);
                assert_eq!(window.requested_size(), None);
            }
        }
    }
}

#[test]
fn expand_preview_retargets_current_pose_without_changing_the_native_spine() {
    use crate::layout::shrink_expand::NormalSizePlan;
    let mut options = Options::default();
    options.layout.border.off = true;
    options.animations.window_movement.0 = niri_config::Animation {
        off: false,
        kind: niri_config::animations::Kind::Easing(niri_config::animations::EasingParams {
            duration_ms: 200,
            curve: niri_config::animations::Curve::Linear,
        }),
    };
    let mut params = TestWindowParams::new(1);
    params.bbox = Rectangle::from_size((120, 80).into());
    let window = TestWindow::new(params);
    let mut clock = Clock::with_time(Duration::ZERO);
    let mut tile = Tile::new(
        window.clone(),
        (1280., 720.).into(),
        1.,
        clock.clone(),
        Rc::new(options),
    );
    tile.set_spine(ShrinkAxis::Horizontal, 40.);
    let native = tile.tile_size();
    let plan = NormalSizePlan {
        allocation: (600., 200.).into(),
        client_size: (600, 200).into(),
        sizing_mode: SizingMode::Normal,
    };
    tile.set_expand_preview(plan, 0.5, true);
    assert_eq!(tile.expand_preview_size(), Some(native));
    clock.set_unadjusted(Duration::from_millis(50));
    let displayed = tile.expand_preview_size().unwrap();
    assert_eq!(displayed, Size::from((105., 85.)));
    tile.set_expand_preview(plan, 0.25, true);
    assert_eq!(tile.expand_preview_size(), Some(displayed));
    clock.set_unadjusted(Duration::from_millis(250));
    tile.advance_animations();
    assert_eq!(tile.expand_preview_size(), Some((150., 50.).into()));
    assert!(!tile.are_transitions_ongoing());
    assert_eq!(tile.tile_size(), native);
    assert_eq!(tile.form(), WindowForm::Spine(ShrinkAxis::Horizontal));
    assert_eq!(window.requested_size(), None);
    tile.clear_expand_preview();
    assert!(tile.expand_preview_size().is_none());
}

#[test]
#[ignore = "requires explicit software EGL live selection handoff rendering"]
fn expand_selection_handoff_retains_visible_content_pose_and_has_a_finite_input_barrier() {
    use crate::layout::shrink_expand::NormalSizePlan;
    use crate::render_helpers::solid_color::SolidColorBuffer;
    use crate::render_helpers::{render_and_download, RenderTarget};
    use smithay::backend::allocator::Fourcc;
    use smithay::backend::renderer::element::Element as _;
    use smithay::backend::renderer::ExportMem as _;
    let mut renderer = spine_test_renderer();
    for scale in [1., 1.25] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            for early_reply in [false, true] {
                let mut options = Options::default();
                options.layout.border.off = true;
                options.layout.focus_ring.off = true;
                let animation = niri_config::Animation {
                    off: false,
                    kind: niri_config::animations::Kind::Easing(
                        niri_config::animations::EasingParams {
                            duration_ms: 200,
                            curve: niri_config::animations::Curve::Linear,
                        },
                    ),
                };
                options.animations.window_movement.0 = animation;
                options.animations.window_resize.anim = animation;
                let mut params = TestWindowParams::new(1);
                params.bbox = Rectangle::from_size((120, 80).into());
                params.rules = Some(ResolvedWindowRules {
                    block_out_from: Some(niri_config::BlockOutFrom::ScreenCapture),
                    ..Default::default()
                });
                let client = TestWindow::new(params);
                *client.0.render_solid.borrow_mut() =
                    Some(SolidColorBuffer::new((120., 80.), [0., 1., 0., 1.]));
                *client.0.render_solid_blocked.borrow_mut() =
                    Some(SolidColorBuffer::new((120., 80.), [0., 0., 0., 1.]));
                let mut clock = Clock::with_time(Duration::ZERO);
                let mut tile = Tile::new(
                    client.clone(),
                    (1280., 720.).into(),
                    scale,
                    clock.clone(),
                    Rc::new(options),
                );
                tile.set_spine(axis, 40.);
                let plan = NormalSizePlan {
                    allocation: (240., 160.).into(),
                    client_size: (240, 160).into(),
                    sizing_mode: SizingMode::Normal,
                };
                tile.set_expand_preview(plan, 0.5, true);
                clock.set_unadjusted(Duration::from_millis(50));
                tile.update_render_elements(false, Rectangle::from_size((1280., 720.).into()));
                let previous = tile
                    .render_expand_preview(
                        RenderCtx {
                            renderer: &mut renderer,
                            target: RenderTarget::Output,
                            xray: None,
                        },
                        (300., 200.).into(),
                        XrayPos::default(),
                        true,
                        &mut |_| {},
                    )
                    .unwrap()
                    .geometry(Scale::from(scale));
                tile.commit_expand_preview();
                tile.prepare_normal_restore(plan);
                tile.rebase_expand_restore_position((-150., -100.).into());
                let transaction = Transaction::new();
                let deadline = transaction.deadline();
                tile.submit_normal_restore(false, transaction, false);
                tile.update_render_elements(true, Rectangle::from_size((1280., 720.).into()));
                let collect = |tile: &Tile<TestWindow>, renderer: &mut _, target| {
                    let mut elements = Vec::new();
                    tile.render(
                        RenderCtx {
                            renderer,
                            target,
                            xray: None,
                        },
                        (450., 300.).into(),
                        XrayPos::default(),
                        false,
                        &mut |elem| elements.push(elem),
                    );
                    elements
                };
                let first = collect(&tile, &mut renderer, RenderTarget::Output);
                assert_eq!(first.len(), 1);
                assert_eq!(first[0].geometry(Scale::from(scale)), previous);
                assert!(tile.normal_restore_input_blocked());
                assert_eq!(client.normal_restore_deadline(), Some(deadline));
                // A later selection cancellation cannot remove an already submitted presentation.
                tile.clear_expand_preview();
                assert!(tile.expand_preview_size().is_some());
                clock.set_unadjusted(Duration::from_millis(if early_reply { 100 } else { 260 }));
                tile.advance_animations();
                tile.update_render_elements(true, Rectangle::from_size((1280., 720.).into()));
                assert_eq!(client.normal_restore_deadline(), Some(deadline));
                assert!(tile.normal_restore_input_blocked());
                assert!(tile.expand_preview_size().is_some());
                let pending_geo = collect(&tile, &mut renderer, RenderTarget::Output)[0]
                    .geometry(Scale::from(scale));
                client.communicate();
                client
                    .0
                    .render_solid
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .resize((240., 160.));
                client
                    .0
                    .render_solid_blocked
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .resize((240., 160.));
                tile.update_window();
                tile.update_render_elements(true, Rectangle::from_size((1280., 720.).into()));
                assert!(!tile.normal_restore_input_blocked());
                if early_reply {
                    assert!(tile.are_transitions_ongoing());
                }
                let output = collect(&tile, &mut renderer, RenderTarget::Output);
                assert_eq!(output[0].geometry(Scale::from(scale)), pending_geo);
                let blocked = collect(&tile, &mut renderer, RenderTarget::ScreenCapture);
                let physical = Size::from((900., 700.)).to_physical_precise_round(scale);
                for (elements, green) in [(output, true), (blocked, false)] {
                    let geometry = elements[0].geometry(Scale::from(scale));
                    let center: Point<i32, smithay::utils::Physical> =
                        geometry.loc + geometry.size.to_point().downscale(2);
                    let download = render_and_download(
                        &mut renderer,
                        physical,
                        Scale::from(scale),
                        Transform::Normal,
                        Fourcc::Abgr8888,
                        elements.into_iter().rev(),
                    )
                    .unwrap();
                    let data = renderer.map_texture(&download).unwrap();
                    let offset = (center.y as usize * physical.w as usize + center.x as usize) * 4;
                    assert_eq!(
                        &data[offset..offset + 4],
                        if green {
                            &[0, 255, 0, 255]
                        } else {
                            &[0, 0, 0, 255]
                        }
                    );
                }
                clock.set_unadjusted(Duration::from_millis(500));
                tile.advance_animations();
                assert!(tile.expand_preview_size().is_none());
                assert_eq!(tile.tile_size(), plan.allocation);
                assert_eq!(tile.form(), WindowForm::Normal);
            }
        }
    }
}

#[test]
fn nearby_preview_view_reveal_preserves_focus_native_requests_query_and_input_owners() {
    for scale in [1., 1.25] {
        let mut options = Options::default();
        options.animations.off = true;
        let mut clock = Clock::with_time(Duration::ZERO);
        let mut layout = Layout::with_options(clock.clone(), options);
        Op::AddScaledOutput {
            id: 1,
            scale,
            layout_config: None,
        }
        .apply(&mut layout);
        for id in 1..=40 {
            Op::AddWindow {
                params: TestWindowParams::new(id),
            }
            .apply(&mut layout);
            Op::Communicate(id).apply(&mut layout);
        }
        for id in 2..=40 {
            assert!(layout.commit_shrink(&id, ShrinkAxis::Horizontal, 32.));
        }
        layout.activate_window(&1);
        clock.set_complete_instantly(true);
        layout.advance_animations();
        clock.set_complete_instantly(false);
        let query = layout
            .active_workspace()
            .unwrap()
            .spine_candidates(Some(&1), |a, b| a.id().cmp(b.id()))
            .unwrap();
        let requests = layout
            .windows()
            .map(|(_, win)| (*win.id(), win.requested_size()))
            .collect::<Vec<_>>();
        let width = 1280. / scale;
        let before = layout
            .active_workspace()
            .unwrap()
            .tiles_with_render_positions()
            .find(|(tile, _, _)| tile.window().id() == &35)
            .unwrap()
            .1;
        assert!(before.x > width);
        assert!(layout.set_expand_preview(&35, 0.75, false));
        // Native zero-duration view animations retain their initial pose within the same cycle.
        clock.set_unadjusted(Duration::from_millis(1));
        layout.advance_animations();
        let ws = layout.active_workspace().unwrap();
        let frame = ws
            .tiles_with_render_positions()
            .find(|(tile, _, _)| tile.window().id() == &35)
            .and_then(|(tile, pos, _)| tile.expand_preview_rect(pos))
            .unwrap();
        assert!(
            frame.loc.x >= -1. / scale && frame.loc.x + frame.size.w <= width + 1. / scale,
            "{frame:?}"
        );
        assert_eq!(*layout.focus().unwrap().id(), 1);
        assert_eq!(
            layout
                .active_workspace()
                .unwrap()
                .spine_candidates(Some(&1), |a, b| a.id().cmp(b.id())),
            Some(query)
        );
        assert_eq!(
            layout
                .windows()
                .map(|(_, win)| (*win.id(), win.requested_size()))
                .collect::<Vec<_>>(),
            requests
        );
        assert!(!layout.set_expand_preview(&35, 0.75, false));
        assert!(layout.interactive_resize_begin(1, ResizeEdge::RIGHT));
        let target = layout
            .active_workspace()
            .unwrap()
            .scrolling()
            .target_view_pos();
        layout.set_expand_preview(&36, 0.75, false);
        assert_eq!(
            layout
                .active_workspace()
                .unwrap()
                .scrolling()
                .target_view_pos(),
            target
        );
        layout.interactive_resize_end(&1);
        layout
            .active_workspace_mut()
            .unwrap()
            .scrolling_mut()
            .view_offset_gesture_begin(true);
        let view = layout.active_workspace().unwrap().scrolling().view_pos();
        layout.set_expand_preview(&37, 0.75, false);
        assert_eq!(
            layout.active_workspace().unwrap().scrolling().view_pos(),
            view
        );
        layout.verify_invariants();
    }
}

#[test]
fn spine_search_native_vertical_members_keep_direct_sides_ahead_of_same_axis_chain() {
    let mut options = Options::default();
    options.animations.off = true;
    let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
    Op::AddOutput(1).apply(&mut layout);
    for id in 1..=4 {
        Op::AddWindow {
            params: TestWindowParams::new(id),
        }
        .apply(&mut layout);
        Op::Communicate(id).apply(&mut layout);
    }
    layout.consume_or_expel_window_left(Some(&2));
    layout.consume_or_expel_window_left(Some(&3));
    assert!(layout.commit_shrink(&2, ShrinkAxis::Vertical, 32.));
    assert!(layout.commit_shrink(&3, ShrinkAxis::Vertical, 32.));
    assert!(layout.commit_shrink(&4, ShrinkAxis::Horizontal, 32.));
    layout.set_window_height(Some(&1), SizeChange::SetFixed(200));
    Op::Communicate(1).apply(&mut layout);
    layout.activate_window(&1);
    let candidates = layout
        .active_workspace()
        .unwrap()
        .spine_candidates(Some(&1), |a, b| a.id().cmp(b.id()))
        .unwrap()
        .candidates;
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| (candidate.id, candidate.distance))
            .collect::<Vec<_>>(),
        [(4, 1), (2, 1), (3, 2)]
    );
    layout.verify_invariants();
}

#[test]
fn spine_drop_hint_and_commit_share_native_plan_preserve_client_and_pointer_ratio() {
    for scale in [1., 1.25] {
        for case in ["both", "other", "neither", "floating", "normal"] {
            let mut options = Options::default();
            options.animations.off = true;
            let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
            layout.clock.set_complete_instantly(true);
            Op::AddScaledOutput {
                id: 1,
                scale,
                layout_config: None,
            }
            .apply(&mut layout);
            Op::AddWindow {
                params: TestWindowParams::new(1),
            }
            .apply(&mut layout);
            Op::Communicate(1).apply(&mut layout);
            let output = layout.outputs().next().unwrap().clone();
            if case != "normal" {
                assert!(layout.commit_shrink(&1, ShrinkAxis::Vertical, 40.));
            }
            if case == "floating" {
                layout.set_window_floating(Some(&1), true);
            }
            let client = layout
                .windows()
                .find(|(_, win)| win.id() == &1)
                .unwrap()
                .1
                .clone();
            let request = client.requested_size();
            layout.advance_animations();
            let start = {
                let ws = layout.active_workspace().unwrap();
                let (tile, pos, _) = ws
                    .tiles_with_render_positions()
                    .find(|(tile, _, _)| tile.window().id() == &1)
                    .unwrap();
                let geometry = tile.move_anchor_geometry();
                pos + geometry.loc + Point::from((geometry.size.w * 0.3, geometry.size.h * 0.6))
            };
            assert!(layout.interactive_move_begin(1, &output, start));
            assert!(layout.interactive_move_update(
                &1,
                (300., 0.).into(),
                output.clone(),
                (500., 250.).into()
            ));
            let Some(InteractiveMoveState::Moving(move_)) = &mut layout.interactive_move else {
                panic!()
            };
            if case == "other" {
                move_.width = ColumnWidth::Fixed(2500.);
            }
            if case == "neither" {
                output.change_current_state(
                    Some(smithay::output::Mode {
                        size: (20, 20).into(),
                        refresh: 60000,
                    }),
                    None,
                    None,
                    None,
                );
                smithay::desktop::layer_map_for_output(&output).arrange();
                layout.update_output_size(&output);
            }
            layout.update_render_elements(None);
            let mon = layout.monitors().find(|mon| mon.output == output).unwrap();
            let hint = mon.insert_hint.as_ref().unwrap();
            let native_position = hint.position;
            assert_eq!(
                native_position,
                if case == "floating" {
                    InsertPosition::Floating
                } else {
                    InsertPosition::NewColumn(0)
                }
            );
            let planned = hint.spine;
            let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move else {
                panic!()
            };
            if case != "normal" {
                assert_eq!(move_.tile.form(), WindowForm::Spine(ShrinkAxis::Vertical));
            }
            let ratio = move_.pointer_ratio_within_window;
            assert!((ratio.0 - 0.3).abs() < 1e-9 && (ratio.1 - 0.6).abs() < 1e-9);
            let expected_axis = if case == "other" {
                ShrinkAxis::Horizontal
            } else {
                ShrinkAxis::Vertical
            };
            if case == "normal" || case == "floating" {
                assert!(planned.is_none());
            } else {
                let plan = planned.unwrap();
                assert_eq!(plan.geometry.axis, expected_axis);
                assert_eq!(plan.fits, case != "neither");
                // Exercise the actual Moving owner's anchor with the planned shape.
                let zoom = layout.overview_zoom();
                let Some(InteractiveMoveState::Moving(move_)) = &mut layout.interactive_move else {
                    panic!()
                };
                let original = move_.tile.spine_geometry().unwrap();
                move_.tile.apply_spine_drop(plan.geometry);
                let geometry = move_.tile.move_anchor_geometry();
                let anchor = move_.tile_render_location(zoom)
                    + (geometry.loc
                        + Point::from((geometry.size.w * ratio.0, geometry.size.h * ratio.1))
                        - move_.tile.render_offset())
                    .upscale(zoom);
                let pixel = 1. / scale;
                assert!((anchor.x - move_.pointer_pos_within_output.x).abs() <= pixel);
                assert!((anchor.y - move_.pointer_pos_within_output.y).abs() <= pixel);
                move_.tile.apply_spine_drop(original);
            }
            layout.interactive_move_end(&1);
            let ws = layout
                .workspaces()
                .find(|(_, _, ws)| ws.has_window(&1))
                .unwrap()
                .2;
            let retained = ws.tiles().find(|tile| tile.window().id() == &1).unwrap();
            if case == "normal" {
                assert_eq!(retained.form(), WindowForm::Normal);
            } else {
                assert_eq!(retained.form(), WindowForm::Spine(expected_axis));
                if let Some(plan) = planned {
                    assert_eq!(retained.tile_size(), plan.geometry.outer_size());
                }
                assert_eq!(client.requested_size(), request);
            }
            assert!(Rc::ptr_eq(&retained.window().0, &client.0));
            assert_eq!(layout.windows().count(), 1);
            layout.verify_invariants();
        }
    }
}

#[test]
fn spine_drop_revalidates_after_layout_change_instead_of_committing_stale_hint() {
    let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), Options::default());
    layout.clock.set_complete_instantly(true);
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    Op::Communicate(1).apply(&mut layout);
    assert!(layout.commit_shrink(&1, ShrinkAxis::Vertical, 40.));
    let output = layout.outputs().next().unwrap().clone();
    assert!(layout.interactive_move_begin(1, &output, (60., 30.).into()));
    assert!(layout.interactive_move_update(
        &1,
        (300., 0.).into(),
        output.clone(),
        (500., 250.).into()
    ));
    layout.update_render_elements(None);
    let old = layout
        .monitors()
        .next()
        .unwrap()
        .insert_hint
        .as_ref()
        .unwrap()
        .spine
        .unwrap();
    assert_eq!(old.geometry.axis, ShrinkAxis::Vertical);
    let Some(InteractiveMoveState::Moving(move_)) = &mut layout.interactive_move else {
        panic!()
    };
    move_.width = ColumnWidth::Fixed(2500.);
    layout.interactive_move_end(&1);
    assert!(layout.is_spine(&1));
    let tile = layout
        .active_workspace()
        .unwrap()
        .tiles()
        .find(|tile| tile.window().id() == &1)
        .unwrap();
    assert_eq!(tile.form(), WindowForm::Spine(ShrinkAxis::Horizontal));
    layout.verify_invariants();
}

#[test]
fn spine_drop_into_shared_and_tabbed_columns_matches_hint_without_expanding() {
    for scale in [1., 1.25] {
        for display in [ColumnDisplay::Normal, ColumnDisplay::Tabbed] {
            for (wide, mode) in [
                (false, SizingMode::Normal),
                (true, SizingMode::Normal),
                (true, SizingMode::Fullscreen),
                (true, SizingMode::Maximized),
            ] {
                let mut options = Options::default();
                options.animations.off = true;
                let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
                layout.clock.set_complete_instantly(true);
                Op::AddScaledOutput {
                    id: 1,
                    scale,
                    layout_config: None,
                }
                .apply(&mut layout);
                for id in [1, 2] {
                    Op::AddWindow {
                        params: TestWindowParams::new(id),
                    }
                    .apply(&mut layout);
                    Op::Communicate(id).apply(&mut layout);
                }
                layout.set_column_display(display);
                layout.set_window_width(
                    Some(&2),
                    SizeChange::SetFixed(if wide { 2500 } else { 600 }),
                );
                match mode {
                    SizingMode::Fullscreen => layout.set_fullscreen(&2, true),
                    SizingMode::Maximized => layout.set_maximized(&2, true),
                    SizingMode::Normal => (),
                }
                Op::Communicate(2).apply(&mut layout);
                assert!(layout.commit_shrink(&1, ShrinkAxis::Vertical, 40.));
                let client = layout
                    .windows()
                    .find(|(_, w)| w.id() == &1)
                    .unwrap()
                    .1
                    .clone();
                let request = client.requested_size();
                let output = layout.outputs().next().unwrap().clone();
                layout.advance_animations();
                assert!(layout.interactive_move_begin(1, &output, (60., 30.).into()));
                assert!(layout.interactive_move_update(
                    &1,
                    (300., 0.).into(),
                    output.clone(),
                    (500., 250.).into()
                ));
                layout.advance_animations();
                // Choose a genuine nearest-gap InColumn result, without forcing InsertHint.
                let ws = layout.active_workspace().unwrap();
                let view_size = output_size(&output);
                let point = (0..view_size.h as i32)
                    .step_by(10)
                    .find_map(|y| {
                        (0..view_size.w as i32).step_by(10).find_map(|x| {
                            let point = Point::from((f64::from(x), f64::from(y)));
                            matches!(
                                ws.scrolling_insert_position(point),
                                InsertPosition::InColumn(0, _)
                            )
                            .then_some(point)
                        })
                    })
                    .expect("native shared-column gap");
                assert!(layout.interactive_move_update(&1, (0., 0.).into(), output, point));
                layout.update_render_elements(None);
                let hint = layout
                    .monitors()
                    .next()
                    .unwrap()
                    .insert_hint
                    .as_ref()
                    .unwrap();
                assert!(matches!(hint.position, InsertPosition::InColumn(0, _)));
                let plan = hint.spine.unwrap();
                if mode == SizingMode::Normal {
                    assert_eq!(
                        plan.geometry.axis,
                        if wide {
                            ShrinkAxis::Horizontal
                        } else {
                            ShrinkAxis::Vertical
                        }
                    );
                }
                let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move else {
                    panic!()
                };
                assert_eq!(move_.tile.form(), WindowForm::Spine(ShrinkAxis::Vertical));
                layout.interactive_move_end(&1);
                let ws = layout
                    .workspaces()
                    .find(|(_, _, ws)| ws.has_window(&1))
                    .unwrap()
                    .2;
                let tile = ws.tiles().find(|tile| tile.window().id() == &1).unwrap();
                assert_eq!(tile.form(), WindowForm::Spine(plan.geometry.axis));
                assert_eq!(tile.tile_size(), plan.geometry.outer_size());
                assert!(Rc::ptr_eq(&tile.window().0, &client.0));
                assert_eq!(client.requested_size(), request);
                assert_eq!(ws.scrolling().columns().count(), 1);
                let neighbor = ws
                    .tiles()
                    .find(|tile| tile.window().id() == &2)
                    .unwrap()
                    .window();
                assert_eq!(
                    neighbor.pending_sizing_mode(),
                    if display == ColumnDisplay::Tabbed {
                        mode
                    } else {
                        SizingMode::Normal
                    }
                );
                assert_eq!(layout.windows().count(), 2);
                layout.verify_invariants();
            }
        }
    }
}

#[test]
fn spine_drop_overview_workspace_gaps_use_native_insertion_and_keep_identity() {
    for scale in [1., 1.25] {
        for gap in ["top", "middle", "bottom"] {
            let mut options = Options::default();
            options.animations.off = true;
            options.layout.empty_workspace_above_first = gap == "top";
            let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
            layout.clock.set_complete_instantly(true);
            Op::AddScaledOutput {
                id: 1,
                scale,
                layout_config: None,
            }
            .apply(&mut layout);
            for id in [1, 2] {
                Op::AddWindow {
                    params: TestWindowParams::new(id),
                }
                .apply(&mut layout);
                Op::Communicate(id).apply(&mut layout);
            }
            let source_idx = usize::from(gap == "top");
            layout.move_to_workspace(Some(&2), source_idx + 1, ActivateWindow::No);
            assert!(layout.commit_shrink(&1, ShrinkAxis::Vertical, 40.));
            let client = layout
                .windows()
                .find(|(_, w)| w.id() == &1)
                .unwrap()
                .1
                .clone();
            let request = client.requested_size();
            let output = layout.outputs().next().unwrap().clone();
            assert!(layout.interactive_move_begin(1, &output, (60., 30.).into()));
            assert!(layout.interactive_move_update(
                &1,
                (300., 0.).into(),
                output.clone(),
                (500., 250.).into()
            ));
            layout.open_overview();
            if gap == "top" {
                layout.switch_workspace(0);
            }
            if gap == "bottom" {
                layout.switch_workspace(source_idx + 1);
            }
            layout.clock.set_unadjusted(Duration::from_millis(1));
            layout.advance_animations();
            let mon = layout.monitors().next().unwrap();
            let desired = match gap {
                "top" => 0,
                "middle" => source_idx + 1,
                "bottom" => mon.workspaces.len() - 1,
                _ => unreachable!(),
            };
            let logical_size = output_size(&output);
            let point = (0..logical_size.h as i32)
                .find_map(|y| {
                    let point = Point::from((logical_size.w / 2., f64::from(y)));
                    (mon.insert_position(point).0 == InsertWorkspace::NewAt(desired))
                        .then_some(point)
                })
                .unwrap_or_else(|| panic!("visible native {gap} gap at scale {scale}"));
            let old_workspace_ids = mon.workspaces.iter().map(|ws| ws.id()).collect::<Vec<_>>();
            assert!(layout.interactive_move_update(&1, (0., 0.).into(), output, point));
            layout.update_render_elements(None);
            let hint = layout
                .monitors()
                .next()
                .unwrap()
                .insert_hint
                .as_ref()
                .unwrap();
            assert_eq!(hint.workspace, InsertWorkspace::NewAt(desired));
            assert_eq!(hint.position, InsertPosition::NewColumn(0));
            let plan = hint.spine.unwrap();
            layout.interactive_move_end(&1);
            let ws = layout
                .workspaces()
                .find(|(_, _, ws)| ws.has_window(&1))
                .unwrap()
                .2;
            let tile = ws.tiles().find(|tile| tile.window().id() == &1).unwrap();
            assert_eq!(tile.form(), WindowForm::Spine(plan.geometry.axis));
            assert_eq!(tile.tile_size(), plan.geometry.outer_size());
            assert_eq!(client.requested_size(), request);
            assert!(Rc::ptr_eq(&tile.window().0, &client.0));
            if gap == "middle" {
                assert!(!old_workspace_ids.contains(&ws.id()));
            } else {
                assert!(old_workspace_ids.contains(&ws.id()));
            }
            assert!(layout.is_overview_open());
            assert_eq!(layout.windows().count(), 2);
            layout.verify_invariants();
        }
    }
}

#[test]
fn spine_drop_cross_output_and_output_loss_keep_native_owner_and_client_identity() {
    for case in ["cross", "source-lost", "target-lost", "all-lost"] {
        let mut options = Options::default();
        options.animations.off = true;
        let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
        layout.clock.set_complete_instantly(true);
        for (id, scale) in [(1, 1.), (2, 1.25)] {
            Op::AddScaledOutput {
                id,
                scale,
                layout_config: None,
            }
            .apply(&mut layout);
        }
        Op::AddWindow {
            params: TestWindowParams::new(1),
        }
        .apply(&mut layout);
        Op::Communicate(1).apply(&mut layout);
        assert!(layout.commit_shrink(&1, ShrinkAxis::Vertical, 40.));
        let client = layout.windows().next().unwrap().1.clone();
        let request = client.requested_size();
        let source = layout
            .outputs()
            .find(|o| o.name() == "output1")
            .unwrap()
            .clone();
        let target = layout
            .outputs()
            .find(|o| o.name() == "output2")
            .unwrap()
            .clone();
        assert!(layout.interactive_move_begin(1, &source, (60., 30.).into()));
        assert!(layout.interactive_move_update(
            &1,
            (300., 0.).into(),
            target.clone(),
            (400., 250.).into()
        ));
        let Some(InteractiveMoveState::Moving(move_)) = &mut layout.interactive_move else {
            panic!()
        };
        move_.width = ColumnWidth::Fixed(2500.);
        layout.update_render_elements(None);
        let plan = layout
            .monitor_for_output(&target)
            .unwrap()
            .insert_hint
            .as_ref()
            .unwrap()
            .spine
            .unwrap();
        assert_eq!(plan.geometry.axis, ShrinkAxis::Horizontal);
        if case == "source-lost" || case == "all-lost" {
            layout.remove_output(&source);
        }
        if case == "target-lost" || case == "all-lost" {
            layout.remove_output(&target);
        }
        layout.interactive_move_end(&1);
        assert!(layout.interactive_move.is_none());
        let (output, tile) = layout
            .workspaces()
            .find_map(|(output, _, ws)| {
                ws.tiles()
                    .find(|tile| tile.window().id() == &1)
                    .map(|tile| (output, tile))
            })
            .unwrap();
        assert_eq!(tile.form(), WindowForm::Spine(ShrinkAxis::Horizontal));
        assert_eq!(tile.spine_geometry().unwrap().thickness(), 40.);
        assert_eq!(client.requested_size(), request);
        assert!(Rc::ptr_eq(&tile.window().0, &client.0));
        if case == "cross" || case == "source-lost" {
            assert_eq!(output.map(|mon| &mon.output), Some(&target));
            assert_eq!(tile.tile_size(), plan.geometry.outer_size());
        } else if case == "target-lost" {
            assert_eq!(output.map(|mon| &mon.output), Some(&source));
        } else {
            assert!(output.is_none());
        }
        assert_eq!(layout.windows().count(), 1);
        layout.verify_invariants();
    }
}

#[test]
#[ignore = "requires an explicit software EGL drop-hint rendering run"]
fn spine_drop_workspace_gap_hint_renders_planned_aspect_without_client_sampling() {
    use crate::render_helpers::render_and_download;
    use smithay::backend::allocator::Fourcc;
    use smithay::backend::renderer::element::Element as _;
    use smithay::backend::renderer::ExportMem as _;
    let mut renderer = spine_test_renderer();
    for scale in [1., 1.25] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            let mut options = Options::default();
            options.animations.off = true;
            options.layout.insert_hint.color =
                niri_config::Color::from_array_unpremul([1., 0., 0., 1.]);
            options.layout.insert_hint.gradient = None;
            let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
            layout.clock.set_complete_instantly(true);
            Op::AddScaledOutput {
                id: 1,
                scale,
                layout_config: None,
            }
            .apply(&mut layout);
            Op::AddWindow {
                params: TestWindowParams::new(1),
            }
            .apply(&mut layout);
            Op::Communicate(1).apply(&mut layout);
            assert!(layout.commit_shrink(&1, axis, 40.));
            let client = layout.windows().next().unwrap().1.clone();
            let request = client.requested_size();
            let render_calls = client.0.render_normal_calls.get();
            let output = layout.outputs().next().unwrap().clone();
            assert!(layout.interactive_move_begin(1, &output, (60., 30.).into()));
            assert!(layout.interactive_move_update(
                &1,
                (300., 0.).into(),
                output.clone(),
                (500., 250.).into()
            ));
            layout.open_overview();
            layout.clock.set_unadjusted(Duration::from_millis(1));
            layout.advance_animations();
            let view_size = output_size(&output);
            let mon = layout.monitors().next().unwrap();
            let point = (0..view_size.h as i32)
                .find_map(|y| {
                    let point = Point::from((view_size.w / 2., f64::from(y)));
                    matches!(mon.insert_position(point).0, InsertWorkspace::NewAt(_))
                        .then_some(point)
                })
                .expect("visible native Overview gap");
            assert!(layout.interactive_move_update(&1, (0., 0.).into(), output, point));
            layout.update_render_elements(None);
            let mon = layout.monitors().next().unwrap();
            let size = mon
                .insert_hint
                .as_ref()
                .unwrap()
                .spine
                .unwrap()
                .geometry
                .outer_size();
            let mut elements = Vec::new();
            mon.render_insert_hint_between_workspaces(&mut renderer, &mut |elem| {
                elements.push(elem)
            });
            assert!(!elements.is_empty());
            let physical = view_size.to_physical_precise_round(scale);
            let bounds = elements
                .iter()
                .map(|elem| elem.geometry(Scale::from(scale)))
                .reduce(|a, b| a.merge(b))
                .unwrap();
            let ratio_error =
                (f64::from(bounds.size.w) * size.h - f64::from(bounds.size.h) * size.w).abs();
            assert!(
                ratio_error <= (size.w + size.h) * 2.,
                "rendered {bounds:?}, plan {size:?}"
            );
            let download = render_and_download(
                &mut renderer,
                physical,
                Scale::from(scale),
                Transform::Normal,
                Fourcc::Abgr8888,
                elements.into_iter().rev(),
            )
            .unwrap();
            let pixels = renderer.map_texture(&download).unwrap();
            let center = bounds.loc + bounds.size.to_point().downscale(2);
            assert!(Rectangle::from_size(physical).contains(center));
            let offset = (center.y as usize * physical.w as usize + center.x as usize) * 4;
            assert_eq!(&pixels[offset..offset + 4], &[255, 0, 0, 255]);
            assert_eq!(client.0.render_normal_calls.get(), render_calls);
            assert_eq!(client.requested_size(), request);
            layout.interactive_move_end(&1);
            layout.verify_invariants();
        }
    }
}

#[test]
fn spine_drop_live_layout_changes_replan_without_replacing_the_native_gap() {
    for scale in [1., 1.25] {
        for shared in [false, true] {
            for change in [
                "global-border",
                "global-thickness",
                "output-override",
                "workspace-override",
            ] {
                let mut options = Options::default();
                options.animations.off = true;
                let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
                layout.clock.set_complete_instantly(true);
                Op::AddScaledOutput {
                    id: 1,
                    scale,
                    layout_config: None,
                }
                .apply(&mut layout);
                layout.set_workspace_name("ws1".into(), None);
                for id in [1, 2] {
                    Op::AddWindow {
                        params: TestWindowParams::new(id),
                    }
                    .apply(&mut layout);
                    Op::Communicate(id).apply(&mut layout);
                }
                assert!(layout.commit_shrink(&1, ShrinkAxis::Vertical, 40.));
                let client = layout
                    .windows()
                    .find(|(_, w)| w.id() == &1)
                    .unwrap()
                    .1
                    .clone();
                let request = client.requested_size();
                let output = layout.outputs().next().unwrap().clone();
                assert!(layout.interactive_move_begin(1, &output, (60., 30.).into()));
                assert!(layout.interactive_move_update(
                    &1,
                    (300., 0.).into(),
                    output.clone(),
                    (500., 250.).into()
                ));
                layout.advance_animations();
                let ws = layout.active_workspace().unwrap();
                let point = if shared {
                    let size = output_size(&output);
                    (0..size.h as i32)
                        .step_by(10)
                        .find_map(|y| {
                            (0..size.w as i32).step_by(10).find_map(|x| {
                                let point = Point::from((f64::from(x), f64::from(y)));
                                matches!(
                                    ws.scrolling_insert_position(point),
                                    InsertPosition::InColumn(0, _)
                                )
                                .then_some(point)
                            })
                        })
                        .unwrap()
                } else {
                    (0., 250.).into()
                };
                assert!(layout.interactive_move_update(&1, (0., 0.).into(), output.clone(), point));
                layout.update_render_elements(None);
                let old = layout
                    .monitors()
                    .next()
                    .unwrap()
                    .insert_hint
                    .as_ref()
                    .unwrap()
                    .spine
                    .unwrap();
                let part = niri_config::LayoutPart {
                    border: Some(niri_config::BorderRule {
                        on: true,
                        width: Some(FloatOrInt(30.)),
                        ..Default::default()
                    }),
                    gaps: Some(FloatOrInt(32.)),
                    ..Default::default()
                };
                match change {
                    "global-border" => {
                        let mut options = (*layout.options).clone();
                        options.layout.border.off = false;
                        options.layout.border.width = 30.;
                        options.layout.gaps = 32.;
                        layout.update_options(options);
                    }
                    "global-thickness" => {
                        let mut options = (*layout.options).clone();
                        options.shrink_expand.thickness = 70.;
                        layout.update_options(options);
                    }
                    "output-override" => Op::UpdateOutputLayoutConfig {
                        id: 1,
                        layout_config: Some(Box::new(part)),
                    }
                    .apply(&mut layout),
                    "workspace-override" => Op::UpdateWorkspaceLayoutConfig {
                        ws_name: 1,
                        layout_config: Some(Box::new(part)),
                    }
                    .apply(&mut layout),
                    _ => unreachable!(),
                }
                layout.update_render_elements(None);
                let mon = layout.monitors().next().unwrap();
                let hint = mon.insert_hint.as_ref().unwrap();
                let (native_ws, geo) = mon.insert_position(point);
                assert_eq!(hint.workspace, native_ws);
                let InsertWorkspace::Existing(id) = native_ws else {
                    panic!()
                };
                let ws = &mon.workspaces[mon.idx_of_ws(id).unwrap()];
                assert_eq!(
                    hint.position,
                    ws.scrolling_insert_position((point - geo.loc).downscale(mon.overview_zoom()))
                );
                let plan = hint.spine.unwrap();
                assert_ne!(
                    old.geometry.outer_size(),
                    plan.geometry.outer_size(),
                    "{change} shared={shared} scale={scale}"
                );
                let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move else {
                    panic!()
                };
                assert_eq!(move_.tile.form(), WindowForm::Spine(ShrinkAxis::Vertical));
                let raw_thickness = move_.tile.spine_geometry().unwrap().thickness();
                assert_eq!(
                    raw_thickness,
                    if change == "global-thickness" {
                        70.
                    } else {
                        40.
                    }
                );
                layout.interactive_move_end(&1);
                let tile = layout
                    .workspaces()
                    .find(|(_, _, ws)| ws.has_window(&1))
                    .unwrap()
                    .2
                    .tiles()
                    .find(|tile| tile.window().id() == &1)
                    .unwrap();
                assert_eq!(
                    tile.tile_size(),
                    plan.geometry.outer_size(),
                    "{change} shared={shared} scale={scale}"
                );
                assert_eq!(tile.spine_geometry().unwrap().thickness(), raw_thickness);
                assert_eq!(client.requested_size(), request);
                assert!(Rc::ptr_eq(&tile.window().0, &client.0));
                layout.verify_invariants();
            }
        }
    }
}

#[test]
#[ignore = "requires an explicit software EGL existing-workspace hint run"]
fn spine_drop_existing_workspace_hint_renders_native_planned_size_and_zoom() {
    use crate::render_helpers::{render_and_download, RenderTarget};
    use smithay::backend::allocator::Fourcc;
    use smithay::backend::renderer::element::Element as _;
    use smithay::backend::renderer::ExportMem as _;
    let mut renderer = spine_test_renderer();
    for scale in [1., 1.25] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            for shared in [false, true] {
                for overview in [false, true] {
                    let mut options = Options::default();
                    options.animations.off = true;
                    options.layout.border.off = true;
                    options.layout.focus_ring.off = true;
                    options.layout.insert_hint.color = "#ff0000".parse().unwrap();
                    options.layout.insert_hint.gradient = None;
                    let mut layout =
                        Layout::with_options(Clock::with_time(Duration::ZERO), options);
                    layout.clock.set_complete_instantly(true);
                    Op::AddScaledOutput {
                        id: 1,
                        scale,
                        layout_config: None,
                    }
                    .apply(&mut layout);
                    for id in 1..=if shared { 2 } else { 1 } {
                        Op::AddWindow {
                            params: TestWindowParams::new(id),
                        }
                        .apply(&mut layout);
                        Op::Communicate(id).apply(&mut layout);
                    }
                    assert!(layout.commit_shrink(&1, axis, 40.));
                    let client = layout
                        .windows()
                        .find(|(_, w)| w.id() == &1)
                        .unwrap()
                        .1
                        .clone();
                    let calls = client.0.render_normal_calls.get();
                    let request = client.requested_size();
                    let output = layout.outputs().next().unwrap().clone();
                    assert!(layout.interactive_move_begin(1, &output, (60., 30.).into()));
                    assert!(layout.interactive_move_update(
                        &1,
                        (300., 0.).into(),
                        output.clone(),
                        (500., 250.).into()
                    ));
                    if overview {
                        layout.open_overview();
                    }
                    layout.clock.set_unadjusted(Duration::from_millis(1));
                    layout.advance_animations();
                    let mon = layout.monitors().next().unwrap();
                    let view_size = output_size(&output);
                    let point = (0..view_size.h as i32)
                        .step_by(5)
                        .find_map(|y| {
                            (0..view_size.w as i32).step_by(5).find_map(|x| {
                                let point = Point::from((f64::from(x), f64::from(y)));
                                let (InsertWorkspace::Existing(id), geo) =
                                    mon.insert_position(point)
                                else {
                                    return None;
                                };
                                let ws = &mon.workspaces[mon.idx_of_ws(id).unwrap()];
                                let position = ws.scrolling_insert_position(
                                    (point - geo.loc).downscale(mon.overview_zoom()),
                                );
                                (if shared {
                                    matches!(position, InsertPosition::InColumn(0, _))
                                } else {
                                    position == InsertPosition::NewColumn(0)
                                })
                                .then_some(point)
                            })
                        })
                        .unwrap();
                    assert!(layout.interactive_move_update(&1, (0., 0.).into(), output, point));
                    layout.update_render_elements(None);
                    let mon = layout.monitors().next().unwrap();
                    let size = mon
                        .insert_hint
                        .as_ref()
                        .unwrap()
                        .spine
                        .unwrap()
                        .geometry
                        .outer_size();
                    let mut elements = Vec::new();
                    // Mock normal clients have no render buffer; the real monitor path supplies the hint.
                    mon.render_workspaces(
                        RenderCtx {
                            renderer: &mut renderer,
                            target: RenderTarget::Output,
                            xray: None,
                        },
                        false,
                        &mut |elem| elements.push(elem),
                    );
                    let bounds = elements
                        .iter()
                        .map(|elem| elem.geometry(Scale::from(scale)))
                        .reduce(|a, b| a.merge(b))
                        .unwrap();
                    let expected: Size<i32, smithay::utils::Physical> = size
                        .upscale(mon.overview_zoom())
                        .to_physical_precise_round(scale);
                    assert!((bounds.size.w - expected.w).abs() <= 2 && (bounds.size.h - expected.h).abs() <= 2, "actual={bounds:?}, expected={expected:?}, scale={scale}, shared={shared}, overview={overview}");
                    let physical = view_size.to_physical_precise_round(scale);
                    let download = render_and_download(
                        &mut renderer,
                        physical,
                        Scale::from(scale),
                        Transform::Normal,
                        Fourcc::Abgr8888,
                        elements.into_iter().rev(),
                    )
                    .unwrap();
                    let pixels = renderer.map_texture(&download).unwrap();
                    let center = bounds.loc + bounds.size.to_point().downscale(2);
                    assert!(Rectangle::from_size(physical).contains(center));
                    let offset = (center.y as usize * physical.w as usize + center.x as usize) * 4;
                    assert_eq!(&pixels[offset..offset + 4], &[255, 0, 0, 255]);
                    assert_eq!(client.0.render_normal_calls.get(), calls);
                    assert_eq!(client.requested_size(), request);
                    layout.interactive_move_end(&1);
                    layout.verify_invariants();
                }
            }
        }
    }
}

#[test]
fn spine_move_keeps_native_dnd_view_lock_and_edge_scroll_identical_to_normal() {
    let trace = |spine: bool| {
        let mut options = Options::default();
        options.animations.off = true;
        let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
        layout.clock.set_complete_instantly(true);
        Op::AddOutput(1).apply(&mut layout);
        for id in 1..=5 {
            Op::AddWindow {
                params: TestWindowParams::new(id),
            }
            .apply(&mut layout);
            Op::Communicate(id).apply(&mut layout);
        }
        layout.activate_window(&2);
        if spine {
            assert!(layout.commit_shrink(&1, ShrinkAxis::Vertical, 40.));
        }
        layout.clock.set_unadjusted(Duration::from_millis(1));
        layout.advance_animations();
        let output = layout.outputs().next().unwrap().clone();
        assert!(layout.interactive_move_begin(1, &output, (60., 30.).into()));
        assert!(layout
            .active_workspace()
            .unwrap()
            .scrolling()
            .view_offset()
            .is_dnd_scroll());
        let right_edge = (output_size(&output).w - 1., 300.).into();
        assert!(layout.interactive_move_update(&1, (300., 0.).into(), output, right_edge));
        let mut positions = Vec::new();
        for ms in (20..=1000).step_by(20) {
            layout.clock.set_unadjusted(Duration::from_millis(ms));
            layout.advance_animations();
            let ws = layout.active_workspace().unwrap();
            assert!(ws.scrolling().view_offset().is_dnd_scroll());
            assert_eq!(ws.active_window().unwrap().id(), &2);
            positions.push(ws.scrolling().view_pos());
            assert!(matches!(
                layout.interactive_move,
                Some(InteractiveMoveState::Moving(_))
            ));
            assert_eq!(layout.is_spine(&1), spine);
        }
        assert!(positions.last().unwrap() > &positions[0]);
        layout.interactive_move_end(&1);
        assert!(layout
            .workspaces()
            .all(|(_, _, ws)| !ws.scrolling().view_offset().is_dnd_scroll()));
        assert_eq!(layout.is_spine(&1), spine);
        assert_eq!(layout.windows().count(), 5);
        layout.verify_invariants();
        positions
    };
    let normal = trace(false);
    let spine = trace(true);
    assert_eq!(normal.len(), spine.len());
    for (normal, spine) in normal.into_iter().zip(spine) {
        assert!((normal - spine).abs() < 1e-9);
    }
}
