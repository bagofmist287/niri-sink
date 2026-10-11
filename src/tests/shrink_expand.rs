use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_toplevel;
use smithay::reexports::wayland_server::Resource as _;
use wayland_client::Proxy as _;

use super::Fixture;
use crate::layout::shrink_expand::{ShrinkAxis, WindowForm};
use crate::layout::LayoutElement as _;
use crate::layout::SizingMode;

#[test]
fn initial_shrink_waits_for_windowed_fullscreen_unset_commit() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();
    let surface = fixture.client(client).create_window().surface.clone();
    fixture.client(client).window(&surface).commit();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    let id = fixture
        .niri()
        .layout
        .windows()
        .next()
        .unwrap()
        .1
        .window
        .clone();
    fixture.niri().layout.toggle_windowed_fullscreen(&id);
    assert!(fixture.niri().layout.shrink_plans(&id, 40.).is_none());
    fixture.double_roundtrip(client);
    fixture
        .client(client)
        .window(&surface)
        .ack_last_and_commit();
    fixture.double_roundtrip(client);
    assert!(fixture.niri().layout.shrink_plans(&id, 40.).is_none());
    fixture.niri().layout.toggle_windowed_fullscreen(&id);
    fixture.double_roundtrip(client);
    // Unset is only pending; the committed client still carries the fullscreen state.
    assert!(fixture.niri().layout.shrink_plans(&id, 40.).is_none());
    fixture
        .client(client)
        .window(&surface)
        .ack_last_and_commit();
    fixture.double_roundtrip(client);
    assert!(fixture
        .niri()
        .layout
        .shrink_plans(&id, 40.)
        .unwrap()
        .first()
        .is_some());
    assert_eq!(fixture.niri().layout.windows().count(), 1);
}

#[test]
fn passive_spine_tab_insertion_does_not_configure_hidden_client_into_container_mode() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            let mut config = niri_config::Config::default();
            config.layout.default_column_display = niri_ipc::ColumnDisplay::Tabbed;
            let mut fixture = Fixture::with_config(config);
            fixture.add_output(1, (1280, 720));
            let client = fixture.add_client();
            let mut surfaces = Vec::new();
            for _ in 0..2 {
                let surface = fixture.client(client).create_window().surface.clone();
                fixture.client(client).window(&surface).commit();
                fixture.roundtrip(client);
                let window = fixture.client(client).window(&surface);
                window.attach_new_buffer();
                window.set_size(100, 100);
                window.ack_last_and_commit();
                fixture.double_roundtrip(client);
                surfaces.push(surface);
            }
            let target = &surfaces[0];
            let spine = &surfaces[1];
            let find_id =
                |niri: &crate::niri::Niri,
                 surface: &wayland_client::protocol::wl_surface::WlSurface| {
                    niri.layout
                        .windows()
                        .find(|(_, mapped)| {
                            mapped.toplevel().wl_surface().id().protocol_id()
                                == surface.id().protocol_id()
                        })
                        .unwrap()
                        .1
                        .window
                        .clone()
                };
            let target_id = find_id(fixture.niri(), target);
            let spine_id = find_id(fixture.niri(), spine);
            let target_window = fixture.client(client).window(target);
            if mode.is_fullscreen() {
                target_window.xdg_toplevel.set_fullscreen(None);
            } else {
                target_window.xdg_toplevel.set_maximized();
            }
            fixture.double_roundtrip(client);
            let window = fixture.client(client).window(target);
            let state = if mode.is_fullscreen() {
                xdg_toplevel::State::Fullscreen
            } else {
                xdg_toplevel::State::Maximized
            };
            assert!(window
                .configures_received
                .last()
                .unwrap()
                .1
                .states
                .contains(&state));
            window.ack_last_and_commit();
            fixture.double_roundtrip(client);
            let window = fixture.client(client).window(spine);
            let previous_client_size = window.configures_received.last().unwrap().1.size;
            let _ = window.recent_configures();
            let niri = fixture.niri();
            let workspace = niri
                .layout
                .workspaces_mut()
                .find(|ws| ws.has_window(&spine_id))
                .unwrap();
            // Internal E0 conversion: user actions and the operation coordinator are not wired yet.
            assert!(workspace
                .tiles_mut()
                .find(|tile| tile.window().window == spine_id)
                .unwrap()
                .set_spine(axis, 40.));
            workspace.scrolling_mut().update_window(&spine_id, None);
            niri.layout.consume_or_expel_window_left(Some(&spine_id));
            fixture.double_roundtrip(client);
            let window = fixture.client(client).window(spine);
            for configure in window.recent_configures() {
                assert!(!configure.states.contains(&xdg_toplevel::State::Fullscreen));
                assert!(!configure.states.contains(&xdg_toplevel::State::Maximized));
                assert!(!configure.states.contains(&xdg_toplevel::State::Activated));
                assert_eq!(configure.size, previous_client_size);
            }
            let niri = fixture.niri();
            let workspace = niri
                .layout
                .workspaces()
                .find(|(_, _, ws)| ws.has_window(&spine_id))
                .unwrap()
                .2;
            assert_eq!(workspace.scrolling().columns().count(), 1);
            assert_eq!(workspace.tiles().count(), 2);
            let book = workspace
                .tiles()
                .find(|tile| tile.window().window == spine_id)
                .unwrap();
            let size = book.tile_size();
            assert_eq!(book.form(), WindowForm::Spine(axis));
            assert_eq!(book.sizing_mode(), SizingMode::Normal);
            assert_eq!(book.window().pending_sizing_mode(), SizingMode::Normal);
            assert_eq!(book.window().sizing_mode(), SizingMode::Normal);
            assert_eq!(
                workspace
                    .tiles()
                    .find(|tile| tile.window().window == target_id)
                    .unwrap()
                    .window()
                    .sizing_mode(),
                mode
            );
            assert!(niri.layout.focus().is_none());
            // A hidden buffer commit must not take over the independent outer allocation.
            let window = fixture.client(client).window(spine);
            window.set_size(200, 150);
            window.ack_last_and_commit();
            fixture.double_roundtrip(client);
            let niri = fixture.niri();
            let book = niri
                .layout
                .workspaces()
                .flat_map(|(_, _, ws)| ws.tiles())
                .find(|tile| tile.window().window == spine_id)
                .unwrap();
            assert_eq!(book.tile_size(), size);
            assert_eq!(book.window().sizing_mode(), SizingMode::Normal);
            assert_eq!(niri.layout.windows().count(), 2);
            fixture.client(client).window(target).attach_null();
            fixture.client(client).window(target).commit();
            fixture.double_roundtrip(client);
            let niri = fixture.niri();
            assert_eq!(niri.layout.windows().count(), 1);
            let book = niri
                .layout
                .workspaces()
                .flat_map(|(_, _, ws)| ws.tiles())
                .find(|tile| tile.window().window == spine_id)
                .unwrap();
            assert_eq!(book.form(), WindowForm::Spine(axis));
            assert_eq!(book.tile_size(), size);
            assert_eq!(book.window().pending_sizing_mode(), SizingMode::Normal);
        }
    }
}

#[test]
fn initial_shrink_rejects_real_transients_with_mapped_or_unmapped_parent() {
    for map_parent in [false, true] {
        let mut fixture = Fixture::new();
        fixture.add_output(1, (1280, 720));
        let client = fixture.add_client();
        let parent = fixture.client(client).create_window().surface.clone();
        fixture.client(client).window(&parent).commit();
        fixture.roundtrip(client);
        if map_parent {
            let window = fixture.client(client).window(&parent);
            window.attach_new_buffer();
            window.set_size(100, 100);
            window.ack_last_and_commit();
            fixture.double_roundtrip(client);
        }
        let parent_top = fixture.client(client).window(&parent).xdg_toplevel.clone();
        let child = fixture.client(client).create_window().surface.clone();
        let window = fixture.client(client).window(&child);
        window.xdg_toplevel.set_parent(Some(&parent_top));
        window.commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&child);
        window.attach_new_buffer();
        window.set_size(100, 100);
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);

        let niri = fixture.niri();
        let mapped = niri
            .layout
            .windows()
            .find(|(_, mapped)| {
                mapped.toplevel().wl_surface().id().protocol_id() == child.id().protocol_id()
            })
            .unwrap()
            .1;
        let id = mapped.window.clone();
        let size = mapped.expected_size();
        assert!(mapped.has_parent());
        assert!(niri.layout.shrink_plans(&id, 40.).is_none());
        assert_eq!(mapped.expected_size(), size);
        assert_eq!(
            niri.layout.windows().count(),
            if map_parent { 2 } else { 1 }
        );
        if map_parent {
            let parent = niri
                .layout
                .windows()
                .find(|(_, mapped)| {
                    mapped.toplevel().wl_surface().id().protocol_id() == parent.id().protocol_id()
                })
                .unwrap()
                .1;
            assert!(niri.layout.shrink_plans(&parent.window, 40.).is_none());
        }

        let window = fixture.client(client).window(&child);
        window.xdg_toplevel.set_parent(None);
        window.commit();
        fixture.double_roundtrip(client);
        let niri = fixture.niri();
        let mapped = niri
            .layout
            .windows()
            .find(|(_, mapped)| mapped.window == id)
            .unwrap()
            .1;
        assert!(!mapped.has_parent());
        let size = mapped.expected_size();
        assert!(niri
            .layout
            .shrink_plans(&id, 40.)
            .unwrap()
            .first()
            .is_some());
        assert_eq!(mapped.expected_size(), size);
        assert_eq!(
            niri.layout.windows().count(),
            if map_parent { 2 } else { 1 }
        );
    }
}

#[test]
fn normal_restore_requires_processed_commit_and_reflow_does_not_extend_deadline() {
    for expires in [false, true] {
        let mut fixture = Fixture::new();
        fixture.add_output(1, (1280, 720));
        let client = fixture.add_client();
        let surface = fixture.client(client).create_window().surface.clone();
        fixture.client(client).window(&surface).commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.set_size(100, 100);
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
        let id = fixture
            .niri()
            .layout
            .windows()
            .next()
            .unwrap()
            .1
            .window
            .clone();
        let plan;
        {
            let ws = fixture
                .niri()
                .layout
                .workspaces_mut()
                .find(|ws| ws.has_window(&id))
                .unwrap();
            assert!(ws
                .tiles_mut()
                .find(|t| t.window().window == id)
                .unwrap()
                .set_spine(ShrinkAxis::Horizontal, 40.));
            ws.scrolling_mut().update_window(&id, None);
        }
        let mut config = niri_config::Config::default();
        config.layout.default_column_width = Some(niri_config::PresetSize::Fixed(760));
        fixture.niri_state().reload_config(Ok(config));
        {
            let ws = fixture
                .niri()
                .layout
                .workspaces_mut()
                .find(|ws| ws.has_window(&id))
                .unwrap();
            plan = ws.normal_spine_plans(std::slice::from_ref(&id))[0].1;
            assert_eq!(
                ws.restore_normal_spines(std::slice::from_ref(&id)),
                vec![id.clone()]
            );
            let tile = ws.tiles().find(|t| t.window().window == id).unwrap();
            assert_eq!(tile.form(), WindowForm::Normal);
            assert_eq!(tile.tile_size(), plan.allocation);
            assert!(tile.normal_restore_input_blocked());
            assert!(ws.active_window().is_none());
        }
        fixture.double_roundtrip(client);
        let deadline = fixture
            .niri()
            .layout
            .windows()
            .find(|(_, w)| w.window == id)
            .unwrap()
            .1
            .normal_restore_deadline()
            .unwrap();
        let window = fixture.client(client).window(&surface);
        assert_eq!(
            window.configures_received.last().unwrap().1.size,
            (plan.client_size.w, plan.client_size.h)
        );
        window.ack_last();
        fixture.double_roundtrip(client);
        {
            let ws = fixture
                .niri()
                .layout
                .workspaces_mut()
                .find(|ws| ws.has_window(&id))
                .unwrap();
            assert!(!ws.tiles().next().unwrap().window().normal_restore_ready());
            ws.scrolling_mut().update_window(&id, None);
            assert_eq!(
                ws.tiles()
                    .next()
                    .unwrap()
                    .window()
                    .normal_restore_deadline(),
                Some(deadline)
            );
            assert_eq!(ws.tiles().next().unwrap().tile_size(), plan.allocation);
        }
        if expires {
            std::thread::sleep(
                deadline.saturating_duration_since(std::time::Instant::now())
                    + std::time::Duration::from_millis(10),
            );
            fixture.double_roundtrip(client);
            let ws = fixture
                .niri()
                .layout
                .workspaces_mut()
                .find(|ws| ws.has_window(&id))
                .unwrap();
            assert!(!ws.tiles().next().unwrap().normal_restore_input_blocked());
            assert!(!ws.tiles().next().unwrap().is_normal_restoring());
            assert!(ws.active_window().is_none());
        }
        // The viewport changes only with this surface commit, not with the preceding ack.
        let window = fixture.client(client).window(&surface);
        window.set_size(plan.client_size.w as u16, plan.client_size.h as u16);
        window.commit();
        fixture.double_roundtrip(client);
        let ws = fixture
            .niri()
            .layout
            .workspaces_mut()
            .find(|ws| ws.has_window(&id))
            .unwrap();
        let tile = ws.tiles().next().unwrap();
        assert!(!tile.normal_restore_input_blocked());
        assert!(tile.window().normal_restore_deadline().is_none());
        assert_eq!(tile.window().size(), plan.client_size);
        assert_eq!(ws.windows().count(), 1);
        assert!(ws.active_window().is_none());
        assert!(ws.activate_window(&id));
        assert!(ws.active_window().is_some());
    }
}

#[test]
fn normal_restore_with_matching_geometry_does_not_wait_for_another_buffer() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();
    let surface = fixture.client(client).create_window().surface.clone();
    let window = fixture.client(client).window(&surface);
    window.xdg_toplevel.set_min_size(100, 100);
    window.xdg_toplevel.set_max_size(100, 100);
    window.commit();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    let id = fixture
        .niri()
        .layout
        .windows()
        .next()
        .unwrap()
        .1
        .window
        .clone();
    fixture.niri().layout.set_window_floating(Some(&id), false);
    fixture.double_roundtrip(client);
    let ws = fixture
        .niri()
        .layout
        .workspaces_mut()
        .find(|ws| ws.has_window(&id))
        .unwrap();
    assert!(ws
        .tiles_mut()
        .next()
        .unwrap()
        .set_spine(ShrinkAxis::Vertical, 40.));
    ws.scrolling_mut().update_window(&id, None);
    assert_eq!(
        ws.normal_spine_plans(std::slice::from_ref(&id))[0]
            .1
            .client_size,
        (100, 100).into()
    );
    assert_eq!(
        ws.restore_normal_spines(std::slice::from_ref(&id)),
        vec![id.clone()]
    );
    assert!(!ws.tiles().next().unwrap().normal_restore_input_blocked());
    assert!(ws
        .tiles()
        .next()
        .unwrap()
        .window()
        .normal_restore_deadline()
        .is_none());
    assert!(ws.activate_window(&id));
    assert!(ws.active_window().is_some());
}

#[test]
fn floating_restore_uses_one_shot_client_choice_or_live_rules_and_keeps_position() {
    for explicit in [false, true] {
        let config = niri_config::Config::parse_mem("window-rule { open-floating true; }").unwrap();
        let mut fixture = Fixture::with_config(config);
        fixture.add_output(1, (1280, 720));
        let client = fixture.add_client();
        let surface = fixture.client(client).create_window().surface.clone();
        fixture.client(client).window(&surface).commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.set_size(100, 100);
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
        let id = fixture
            .niri()
            .layout
            .windows()
            .next()
            .unwrap()
            .1
            .window
            .clone();
        {
            let ws = fixture
                .niri()
                .layout
                .workspaces_mut()
                .find(|ws| ws.has_window(&id))
                .unwrap();
            assert!(ws.floating().has_window(&id));
            assert!(ws
                .tiles_mut()
                .next()
                .unwrap()
                .set_spine(ShrinkAxis::Horizontal, 40.));
            ws.update_window(&id, None);
        }
        if explicit {
            let config = niri_config::Config::parse_mem(
                r#"
window-rule {
    open-floating true
    default-column-width { fixed 720; }
    default-window-height { fixed 400; }
}
"#,
            )
            .unwrap();
            fixture.niri_state().reload_config(Ok(config));
        }
        let plan;
        let position;
        {
            let ws = fixture
                .niri()
                .layout
                .workspaces_mut()
                .find(|ws| ws.has_window(&id))
                .unwrap();
            position = ws.floating().tiles_with_offsets().next().unwrap().1;
            plan = ws.normal_spine_plans(std::slice::from_ref(&id))[0].1;
            assert_eq!(
                plan.client_size,
                if explicit {
                    (720, 400).into()
                } else {
                    (0, 0).into()
                }
            );
            assert_eq!(
                ws.restore_normal_spines(std::slice::from_ref(&id)),
                vec![id.clone()]
            );
            assert_eq!(
                ws.floating().tiles_with_offsets().next().unwrap().1,
                position
            );
            assert_eq!(
                ws.tiles().next().unwrap().normal_restore_input_blocked(),
                explicit
            );
        }
        fixture.double_roundtrip(client);
        if explicit {
            let window = fixture.client(client).window(&surface);
            assert_eq!(
                window.configures_received.last().unwrap().1.size,
                (720, 400)
            );
            window.set_size(720, 400);
            window.ack_last_and_commit();
            fixture.double_roundtrip(client);
        }
        let ws = fixture
            .niri()
            .layout
            .workspaces_mut()
            .find(|ws| ws.has_window(&id))
            .unwrap();
        assert!(ws.floating().has_window(&id));
        assert_eq!(
            ws.floating().tiles_with_offsets().next().unwrap().1,
            position
        );
        assert!(!ws.tiles().next().unwrap().normal_restore_input_blocked());
        assert!(ws
            .tiles()
            .next()
            .unwrap()
            .window()
            .normal_restore_deadline()
            .is_none());
        assert_eq!(ws.windows().count(), 1);
        assert!(ws.active_window().is_none());
    }
}

fn explicit_mode_fixture() -> (
    Fixture,
    super::client::ClientId,
    Vec<wayland_client::protocol::wl_surface::WlSurface>,
    Vec<smithay::desktop::Window>,
) {
    let mut config = niri_config::Config::default();
    config.layout.default_column_display = niri_ipc::ColumnDisplay::Tabbed;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();
    let mut surfaces = Vec::new();
    let mut ids = Vec::new();
    for _ in 0..3 {
        let surface = fixture.client(client).create_window().surface.clone();
        fixture.client(client).window(&surface).commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.set_size(100, 100);
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
        let id = fixture
            .niri()
            .layout
            .windows()
            .find(|(_, w)| {
                w.toplevel().wl_surface().id().protocol_id() == surface.id().protocol_id()
            })
            .unwrap()
            .1
            .window
            .clone();
        surfaces.push(surface);
        ids.push(id);
    }
    for id in &ids[1..] {
        let ws = fixture
            .niri()
            .layout
            .workspaces_mut()
            .find(|ws| ws.has_window(id))
            .unwrap();
        assert!(ws
            .tiles_mut()
            .find(|t| t.window().window == *id)
            .unwrap()
            .set_spine(ShrinkAxis::Horizontal, 40.));
        ws.update_window(id, None);
        fixture.niri().layout.consume_or_expel_window_left(Some(id));
    }
    fixture.niri().layout.activate_window(&ids[0]);
    fixture.double_roundtrip(client);
    for surface in &surfaces {
        let _ = fixture.client(client).window(surface).recent_configures();
    }
    (fixture, client, surfaces, ids)
}

#[test]
fn explicit_xdg_duplicate_mode_and_early_unset_restore_every_affected_tab() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        for all_spines in [false, true] {
            let (mut fixture, client, surfaces, ids) = explicit_mode_fixture();
            if all_spines {
                let ws = fixture
                    .niri()
                    .layout
                    .workspaces_mut()
                    .find(|ws| ws.has_window(&ids[0]))
                    .unwrap();
                assert!(ws
                    .tiles_mut()
                    .find(|t| t.window().window == ids[0])
                    .unwrap()
                    .set_spine(ShrinkAxis::Vertical, 40.));
                ws.update_window(&ids[0], None);
            }
            if mode.is_fullscreen() {
                fixture.niri().layout.set_fullscreen(&ids[0], true);
            } else {
                fixture.niri().layout.set_maximized(&ids[0], true);
            }
            fixture.double_roundtrip(client);
            if !all_spines {
                let window = fixture.client(client).window(&surfaces[0]);
                let size = window.configures_received.last().unwrap().1.size;
                window.set_size(size.0 as u16, size.1 as u16);
                window.ack_last_and_commit();
                fixture.double_roundtrip(client);
            }
            assert!(fixture.niri().layout.is_spine(&ids[1]));
            for surface in &surfaces {
                let _ = fixture.client(client).window(surface).recent_configures();
            }
            let window = fixture.client(client).window(&surfaces[0]);
            if mode.is_fullscreen() {
                window.xdg_toplevel.set_fullscreen(None);
            } else {
                window.xdg_toplevel.set_maximized();
            }
            fixture.double_roundtrip(client);
            let mut deadline = None;
            for id in &ids[1..] {
                assert!(!fixture.niri().layout.is_spine(id));
                let mapped = fixture
                    .niri()
                    .layout
                    .windows()
                    .find(|(_, w)| w.window == *id)
                    .unwrap()
                    .1;
                assert_eq!(mapped.pending_sizing_mode(), mode);
                let current_deadline = mapped.normal_restore_deadline().unwrap();
                if let Some(deadline) = deadline {
                    assert_eq!(current_deadline, deadline);
                }
                deadline = Some(current_deadline);
            }
            let state = if mode.is_fullscreen() {
                xdg_toplevel::State::Fullscreen
            } else {
                xdg_toplevel::State::Maximized
            };
            for surface in &surfaces[1..] {
                let configures = fixture
                    .client(client)
                    .window(surface)
                    .recent_configures()
                    .collect::<Vec<_>>();
                assert!(!configures.is_empty());
                assert!(configures
                    .iter()
                    .all(|configure| configure.states.contains(&state)));
            }
            // Reverse before either restored book has acked/committed its new mode.
            let window = fixture.client(client).window(&surfaces[1]);
            if mode.is_fullscreen() {
                window.xdg_toplevel.unset_fullscreen();
            } else {
                window.xdg_toplevel.unset_maximized();
            }
            fixture.double_roundtrip(client);
            for (surface, id) in surfaces.iter().zip(&ids) {
                let window = fixture.client(client).window(surface);
                let configure = &window.configures_received.last().unwrap().1;
                assert!(!configure.states.contains(&xdg_toplevel::State::Fullscreen));
                assert!(!configure.states.contains(&xdg_toplevel::State::Maximized));
                window.set_size(configure.size.0 as u16, configure.size.1 as u16);
                window.ack_last_and_commit();
                fixture.double_roundtrip(client);
                assert!(!fixture.niri().layout.is_spine(id));
            }
            assert_eq!(fixture.niri().layout.windows().count(), 3);
            if all_spines {
                assert!(fixture.niri().layout.focus().is_none());
            } else {
                assert_eq!(fixture.niri().layout.focus().unwrap().window, ids[0]);
            }
        }
    }
}

#[test]
fn explicit_mode_by_id_action_uses_container_toggle_and_focusless_action_does_nothing() {
    use niri_config::Action;
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let (mut fixture, client, surfaces, ids) = explicit_mode_fixture();
        let ws = fixture
            .niri()
            .layout
            .workspaces_mut()
            .find(|ws| ws.has_window(&ids[0]))
            .unwrap();
        assert!(ws
            .tiles_mut()
            .find(|t| t.window().window == ids[0])
            .unwrap()
            .set_spine(ShrinkAxis::Vertical, 40.));
        ws.update_window(&ids[0], None);
        let native_id = fixture
            .niri()
            .layout
            .windows()
            .find(|(_, w)| w.window == ids[0])
            .unwrap()
            .1
            .id()
            .get();
        assert!(fixture.niri().layout.focus().is_none());
        fixture
            .niri_state()
            .do_action(Action::FullscreenWindow, false);
        fixture
            .niri_state()
            .do_action(Action::MaximizeWindowToEdges, false);
        fixture
            .niri_state()
            .do_action(Action::ToggleWindowedFullscreen, false);
        assert!(ids.iter().all(|id| fixture.niri().layout.is_spine(id)));
        if mode.is_fullscreen() {
            fixture.niri().layout.set_fullscreen(&ids[0], true);
        } else {
            fixture.niri().layout.set_maximized(&ids[0], true);
        }
        let action = if mode.is_fullscreen() {
            Action::FullscreenWindowById(native_id)
        } else {
            Action::MaximizeWindowToEdgesById(native_id)
        };
        fixture.niri_state().do_action(action, false);
        fixture.double_roundtrip(client);
        assert!(ids.iter().all(|id| !fixture.niri().layout.is_spine(id)));
        let ws = fixture
            .niri()
            .layout
            .workspaces_mut()
            .find(|ws| ws.has_window(&ids[0]))
            .unwrap();
        let col = ws.scrolling().columns().next().unwrap();
        assert!(!col.is_pending_fullscreen() && !col.is_pending_maximized());
        assert!(fixture.niri().layout.focus().is_none());
        for surface in &surfaces {
            let configure = &fixture
                .client(client)
                .window(surface)
                .configures_received
                .last()
                .unwrap()
                .1;
            assert!(!configure.states.contains(&xdg_toplevel::State::Fullscreen));
            assert!(!configure.states.contains(&xdg_toplevel::State::Maximized));
        }
    }
}

#[test]
fn explicit_windowed_action_with_no_resize_waits_for_processed_flag_commit() {
    use niri_config::Action;
    let config = niri_config::Config::parse_mem("window-rule { open-floating false; min-width 100; max-width 100; min-height 100; max-height 100; }").unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();
    let surface = fixture.client(client).create_window().surface.clone();
    fixture.client(client).window(&surface).commit();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    let mapped = fixture.niri().layout.windows().next().unwrap().1;
    let id = mapped.window.clone();
    let native_id = mapped.id().get();
    let ws = fixture
        .niri()
        .layout
        .workspaces_mut()
        .find(|ws| ws.has_window(&id))
        .unwrap();
    assert!(ws
        .tiles_mut()
        .next()
        .unwrap()
        .set_spine(ShrinkAxis::Vertical, 40.));
    ws.update_window(&id, None);
    fixture
        .niri_state()
        .do_action(Action::ToggleWindowedFullscreenById(native_id), false);
    fixture.double_roundtrip(client);
    assert!(!fixture.niri().layout.is_spine(&id));
    let mapped = fixture.niri().layout.windows().next().unwrap().1;
    assert!(mapped.is_pending_windowed_fullscreen());
    assert!(!mapped.is_windowed_fullscreen());
    assert!(!mapped.normal_restore_ready());
    let deadline = mapped.normal_restore_deadline().unwrap();
    let window = fixture.client(client).window(&surface);
    assert_eq!(
        window.configures_received.last().unwrap().1.size,
        (100, 100)
    );
    assert!(window
        .configures_received
        .last()
        .unwrap()
        .1
        .states
        .contains(&xdg_toplevel::State::Fullscreen));
    window.ack_last();
    fixture.double_roundtrip(client);
    let mapped = fixture.niri().layout.windows().next().unwrap().1;
    assert!(!mapped.normal_restore_ready());
    assert_eq!(mapped.normal_restore_deadline(), Some(deadline));
    fixture.client(client).window(&surface).commit();
    fixture.double_roundtrip(client);
    let mapped = fixture.niri().layout.windows().next().unwrap().1;
    assert!(mapped.is_windowed_fullscreen());
    assert_eq!(mapped.sizing_mode(), SizingMode::Normal);
    assert!(mapped.normal_restore_deadline().is_none());
    assert!(fixture.niri().layout.focus().is_none());
}

#[test]
fn explicit_foreign_handler_mode_requests_restore_tabs_without_activating_books() {
    use crate::protocols::foreign_toplevel::ForeignToplevelHandler;
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let (mut fixture, client, surfaces, ids) = explicit_mode_fixture();
        let surface = fixture
            .niri()
            .layout
            .windows()
            .find(|(_, w)| w.window == ids[0])
            .unwrap()
            .1
            .toplevel()
            .wl_surface()
            .clone();
        if mode.is_fullscreen() {
            ForeignToplevelHandler::set_fullscreen(fixture.niri_state(), surface, None);
        } else {
            ForeignToplevelHandler::set_maximized(fixture.niri_state(), surface);
        }
        fixture.double_roundtrip(client);
        assert!(ids.iter().all(|id| !fixture.niri().layout.is_spine(id)));
        for surface in &surfaces {
            let window = fixture.client(client).window(surface);
            let configure = &window.configures_received.last().unwrap().1;
            let state = if mode.is_fullscreen() {
                xdg_toplevel::State::Fullscreen
            } else {
                xdg_toplevel::State::Maximized
            };
            assert!(configure.states.contains(&state));
            window.set_size(configure.size.0 as u16, configure.size.1 as u16);
            window.ack_last_and_commit();
        }
        fixture.double_roundtrip(client);
        assert_eq!(fixture.niri().layout.focus().unwrap().window, ids[0]);
    }
}

#[test]
fn explicit_xdg_mode_from_floating_book_preserves_native_return_and_other_books() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let (mut fixture, client, surfaces, ids) = explicit_mode_fixture();
        fixture
            .niri()
            .layout
            .set_window_floating(Some(&ids[1]), true);
        fixture.double_roundtrip(client);
        assert!(fixture.niri().layout.is_spine(&ids[1]));
        let ws = fixture
            .niri()
            .layout
            .workspaces_mut()
            .find(|ws| ws.has_window(&ids[1]))
            .unwrap();
        assert!(ws.is_floating(&ids[1]));
        let window = fixture.client(client).window(&surfaces[1]);
        if mode.is_fullscreen() {
            window.xdg_toplevel.set_fullscreen(None);
        } else {
            window.xdg_toplevel.set_maximized();
        }
        fixture.double_roundtrip(client);
        assert!(!fixture.niri().layout.is_spine(&ids[1]));
        assert!(fixture.niri().layout.is_spine(&ids[2]));
        let ws = fixture
            .niri()
            .layout
            .workspaces_mut()
            .find(|ws| ws.has_window(&ids[1]))
            .unwrap();
        assert!(!ws.is_floating(&ids[1]));
        let window = fixture.client(client).window(&surfaces[1]);
        let configure = &window.configures_received.last().unwrap().1;
        let state = if mode.is_fullscreen() {
            xdg_toplevel::State::Fullscreen
        } else {
            xdg_toplevel::State::Maximized
        };
        assert!(configure.states.contains(&state));
        window.set_size(configure.size.0 as u16, configure.size.1 as u16);
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
        let window = fixture.client(client).window(&surfaces[1]);
        if mode.is_fullscreen() {
            window.xdg_toplevel.unset_fullscreen();
        } else {
            window.xdg_toplevel.unset_maximized();
        }
        fixture.double_roundtrip(client);
        let ws = fixture
            .niri()
            .layout
            .workspaces_mut()
            .find(|ws| ws.has_window(&ids[1]))
            .unwrap();
        assert!(ws.is_floating(&ids[1]));
        let window = fixture.client(client).window(&surfaces[1]);
        let configure = &window.configures_received.last().unwrap().1;
        assert!(!configure.states.contains(&xdg_toplevel::State::Fullscreen));
        assert!(!configure.states.contains(&xdg_toplevel::State::Maximized));
        window.set_size(configure.size.0 as u16, configure.size.1 as u16);
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
        assert_eq!(fixture.niri().layout.windows().count(), 3);
        assert!(fixture.niri().layout.is_spine(&ids[2]));
        assert!(!fixture.niri().layout.is_spine(&ids[1]));
    }
}

#[test]
fn explicit_xdg_unfullscreen_of_windowed_client_does_not_expand_sibling_book() {
    use niri_config::Action;
    let (mut fixture, client, surfaces, ids) = explicit_mode_fixture();
    let native_id = fixture
        .niri()
        .layout
        .windows()
        .find(|(_, w)| w.window == ids[1])
        .unwrap()
        .1
        .id()
        .get();
    fixture
        .niri_state()
        .do_action(Action::ToggleWindowedFullscreenById(native_id), false);
    fixture.double_roundtrip(client);
    let window = fixture.client(client).window(&surfaces[1]);
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    assert!(fixture.niri().layout.is_spine(&ids[2]));
    fixture
        .client(client)
        .window(&surfaces[1])
        .xdg_toplevel
        .unset_fullscreen();
    fixture.double_roundtrip(client);
    assert!(fixture.niri().layout.is_spine(&ids[2]));
    fixture
        .client(client)
        .window(&surfaces[1])
        .xdg_toplevel
        .unset_fullscreen();
    fixture.double_roundtrip(client);
    assert!(fixture.niri().layout.is_spine(&ids[2]));
    let window = fixture.client(client).window(&surfaces[1]);
    assert!(!window
        .configures_received
        .last()
        .unwrap()
        .1
        .states
        .contains(&xdg_toplevel::State::Fullscreen));
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    let mapped = fixture
        .niri()
        .layout
        .windows()
        .find(|(_, w)| w.window == ids[1])
        .unwrap()
        .1;
    assert!(!mapped.is_windowed_fullscreen());
    assert!(!mapped.is_pending_windowed_fullscreen());
    fixture
        .client(client)
        .window(&surfaces[1])
        .xdg_toplevel
        .unset_fullscreen();
    fixture
        .client(client)
        .window(&surfaces[1])
        .xdg_toplevel
        .unset_maximized();
    fixture.double_roundtrip(client);
    assert!(fixture.niri().layout.is_spine(&ids[2]));
    assert_eq!(fixture.niri().layout.windows().count(), 3);
}
