use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_toplevel;
use smithay::reexports::wayland_server::Resource as _;
use wayland_client::Proxy as _;

use super::Fixture;
use crate::layout::shrink_expand::{ShrinkAxis, WindowForm};
use crate::layout::LayoutElement as _;
use crate::layout::SizingMode;

fn timed_shrink_fixture(
    count: usize,
    floating: bool,
    delay: u32,
) -> (
    Fixture,
    super::client::ClientId,
    Vec<wayland_client::protocol::wl_surface::WlSurface>,
) {
    let mut config = niri_config::Config::default();
    config.animations.off = true;
    config.shrink_expand.enabled = true;
    config.shrink_expand.shrink_buffer_ms = delay;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();
    let mut surfaces = Vec::new();
    for _ in 0..count {
        let surface = fixture.client(client).create_window().surface.clone();
        fixture.client(client).window(&surface).commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.set_size(300, 200);
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
        if floating {
            let id = fixture.niri().layout.focus().unwrap().window.clone();
            fixture.niri().layout.toggle_window_floating(Some(&id));
            fixture.double_roundtrip(client);
            fixture
                .client(client)
                .window(&surface)
                .ack_last_and_commit();
            fixture.double_roundtrip(client);
        }
        surfaces.push(surface);
    }
    fixture.niri_complete_animations();
    (fixture, client, surfaces)
}

#[test]
fn timed_shrink_repeat_rejects_old_ticket_and_commits_native_focus_handoff() {
    use crate::shrink_expand::{FinishReason, Kind, Phase};
    for floating in [false, true] {
        for count in [1, 2] {
            let (mut fixture, client, surfaces) = timed_shrink_fixture(count, floating, 60000);
            let window = fixture.niri().layout.focus().unwrap().window.clone();
            let old_surface = fixture.niri().keyboard_focus.surface().unwrap().clone();
            let before_size = fixture.niri().layout.focus().unwrap().size();
            let first = fixture
                .niri_state()
                .begin_shrink_preview(Some(ShrinkAxis::Horizontal))
                .unwrap();
            fixture.double_roundtrip(client);
            assert!(!fixture.niri().layout.is_spine(&window));
            assert_eq!(fixture.niri().keyboard_focus.surface(), Some(&old_surface));
            assert_eq!(fixture.niri().layout.focus().unwrap().size(), before_size);
            let second = fixture.niri_state().begin_shrink_preview(None).unwrap();
            assert_eq!(second.id, first.id);
            assert!(second.revision > first.revision);
            assert_eq!(second.kind, Kind::ShrinkTimed(ShrinkAxis::Vertical));
            assert!(fixture
                .niri_state()
                .confirm_shrink_preview(first.ticket())
                .is_none());
            assert!(!fixture.niri().layout.is_spine(&window));
            let submitted = fixture
                .niri_state()
                .confirm_shrink_preview(second.ticket())
                .unwrap();
            assert_eq!(submitted.phase, Phase::Submitted);
            fixture.double_roundtrip(client);
            assert!(fixture.niri().layout.is_spine(&window));
            assert!(fixture.niri().shrink_expand.preview().is_none());
            assert!(fixture
                .niri()
                .shrink_expand
                .records()
                .any(|r| r.id == first.id
                    && r.phase == Phase::Settled
                    && r.reason == Some(FinishReason::Ready)));
            assert!(fixture.niri().shrink_preview_timer.is_none());
            if count == 1 {
                assert!(fixture.niri().keyboard_focus.surface().is_none());
            } else {
                assert!(fixture.niri().keyboard_focus.surface().is_some());
                assert_ne!(fixture.niri().keyboard_focus.surface(), Some(&old_surface));
                let receiver = fixture.niri().layout.focus().unwrap().window.clone();
                assert!(!fixture.niri().layout.is_spine(&receiver));
            }
            assert_eq!(fixture.niri().layout.windows().count(), count);
            assert!(!fixture
                .client(client)
                .window(surfaces.last().unwrap())
                .configures_received
                .last()
                .unwrap()
                .1
                .states
                .contains(&xdg_toplevel::State::Activated));
        }
    }
}

#[test]
fn timed_shrink_actual_zero_timer_submits_and_scope_change_or_disable_cancels() {
    use crate::shrink_expand::{Phase, Rejected};
    let (mut fixture, client, _) = timed_shrink_fixture(1, false, 0);
    let window = fixture.niri().layout.focus().unwrap().window.clone();
    let record = fixture.niri_state().begin_shrink_preview(None).unwrap();
    assert_eq!(record.phase, Phase::Previewing);
    fixture.double_roundtrip(client);
    assert!(fixture.niri().layout.is_spine(&window));
    assert!(fixture
        .niri()
        .shrink_expand
        .records()
        .any(|r| r.id == record.id && r.phase == Phase::Settled));
    for change in ["focus", "disable", "close", "workspace"] {
        let (mut fixture, client, surfaces) = timed_shrink_fixture(2, false, 60000);
        let target = fixture.niri().layout.focus().unwrap().window.clone();
        let record = fixture.niri_state().begin_shrink_preview(None).unwrap();
        match change {
            "focus" => {
                fixture.niri().layout.focus_left();
            }
            "disable" => fixture.niri().config.borrow_mut().shrink_expand.enabled = false,
            "close" => {
                let window = fixture.client(client).window(surfaces.last().unwrap());
                window.xdg_toplevel.destroy();
                window.xdg_surface.destroy();
                window.surface.destroy();
            }
            "workspace" => fixture.niri().layout.switch_workspace_down(),
            _ => unreachable!(),
        }
        fixture.double_roundtrip(client);
        assert!(fixture.niri().shrink_expand.preview().is_none(), "{change}");
        assert!(fixture.niri().shrink_preview_timer.is_none());
        assert!(!fixture.niri().layout.is_spine(&target));
        assert!(fixture
            .niri_state()
            .confirm_shrink_preview(record.ticket())
            .is_none());
        assert!(fixture
            .niri()
            .shrink_expand
            .records()
            .any(|r| r.id == record.id && r.phase == Phase::Cancelled));
        if change == "disable" {
            assert_eq!(
                fixture.niri_state().begin_shrink_preview(None),
                Err(Rejected::Disabled)
            );
        }
    }
}

#[test]
fn timed_shrink_keyboard_suppression_balances_old_and_new_presses_across_escape() {
    use super::client::KeyboardEvent;
    use super::test_input_backend::{TestInputBackend, TestKeyboardKeyEvent};
    use smithay::backend::input::{InputEvent, InputTime, KeyState};
    let (mut fixture, client, surfaces) = timed_shrink_fixture(1, false, 60000);
    let surface = &surfaces[0];
    let _ = fixture
        .client(client)
        .state
        .recent_keyboard_events(surface)
        .count();
    let send = |fixture: &mut Fixture, name: &str, state: KeyState| {
        let keyboard = fixture.niri().seat.get_keyboard().unwrap();
        let code = keyboard.with_xkb_state(fixture.niri_state(), |xkb| {
            let xkb = xkb.xkb().lock().unwrap();
            unsafe { xkb.keymap() }.key_by_name(name).unwrap()
        });
        fixture
            .niri_state()
            .process_input_event(InputEvent::<TestInputBackend>::Keyboard {
                event: TestKeyboardKeyEvent {
                    time: InputTime::from_micros(0),
                    code,
                    state,
                    count: 1,
                },
            });
        fixture.double_roundtrip(client);
    };
    send(&mut fixture, "AC01", KeyState::Pressed);
    assert_eq!(
        fixture
            .client(client)
            .state
            .recent_keyboard_events(surface)
            .filter(|e| matches!(e, KeyboardEvent::Key { .. }))
            .count(),
        1
    );
    fixture.niri_state().begin_shrink_preview(None).unwrap();
    // Release for a press forwarded before the preview must still reach its client.
    send(&mut fixture, "AC01", KeyState::Released);
    assert_eq!(
        fixture
            .client(client)
            .state
            .recent_keyboard_events(surface)
            .filter(|e| matches!(e, KeyboardEvent::Key { .. }))
            .count(),
        1
    );
    send(&mut fixture, "AB05", KeyState::Pressed);
    send(&mut fixture, "ESC", KeyState::Pressed);
    assert!(fixture.niri().shrink_expand.preview().is_none());
    send(&mut fixture, "AB05", KeyState::Released);
    send(&mut fixture, "ESC", KeyState::Released);
    assert_eq!(
        fixture
            .client(client)
            .state
            .recent_keyboard_events(surface)
            .filter(|e| matches!(e, KeyboardEvent::Key { .. }))
            .count(),
        0
    );
    assert!(fixture.niri().suppressed_keys.is_empty());
    assert!(fixture.niri().layout.focus().is_some());
    send(&mut fixture, "AB05", KeyState::Pressed);
    send(&mut fixture, "AB05", KeyState::Released);
    assert_eq!(
        fixture
            .client(client)
            .state
            .recent_keyboard_events(surface)
            .filter(|e| matches!(e, KeyboardEvent::Key { .. }))
            .count(),
        2
    );
}

#[test]
fn real_spine_hit_never_forwards_to_hidden_client_and_static_refresh_stays_idle() {
    use crate::layout::HitType;
    use crate::niri::RedrawState;
    use crate::utils::ResizeEdge;

    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        for floating in [false, true] {
            for scale in [1., 1.25] {
                let mut config = niri_config::Config::default();
                config.animations.off = true;
                let mut fixture = Fixture::with_config(config);
                fixture.add_output(1, (1280, 720));
                let output = fixture.niri_output(1);
                output.change_current_state(
                    None,
                    None,
                    Some(smithay::output::Scale::Fractional(scale)),
                    None,
                );
                fixture.niri().layout.update_output_size(&output);
                let client = fixture.add_client();
                let surface = fixture.client(client).create_window().surface.clone();
                fixture.client(client).window(&surface).commit();
                fixture.roundtrip(client);
                let window = fixture.client(client).window(&surface);
                window.attach_new_buffer();
                window.set_size(900, 700);
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
                if floating {
                    fixture.niri().layout.toggle_window_floating(Some(&id));
                    fixture.double_roundtrip(client);
                    fixture
                        .client(client)
                        .window(&surface)
                        .ack_last_and_commit();
                    fixture.double_roundtrip(client);
                }
                fixture.niri_complete_animations();
                let normal_point = {
                    let workspace = fixture.niri().layout.active_workspace().unwrap();
                    let (tile, pos, visible) =
                        workspace.tiles_with_render_positions().next().unwrap();
                    assert!(visible);
                    pos + tile.window_loc() + smithay::utils::Point::from((10., 10.))
                };
                assert!(fixture
                    .niri()
                    .contents_under(normal_point)
                    .surface
                    .is_some());
                let configure_count = fixture
                    .client(client)
                    .window(&surface)
                    .configures_received
                    .len();
                {
                    let tile = fixture
                        .niri()
                        .layout
                        .workspaces_mut()
                        .find(|ws| ws.has_window(&id))
                        .unwrap()
                        .tiles_mut()
                        .next()
                        .unwrap();
                    assert!(tile.set_spine(axis, 40.));
                }
                fixture.niri().layout.update_window(&id, None);
                fixture.double_roundtrip(client);
                fixture.niri_complete_animations();
                let (pos, size) = {
                    let workspace = fixture.niri().layout.active_workspace().unwrap();
                    let (tile, pos, _) = workspace.tiles_with_render_positions().next().unwrap();
                    assert!(!tile.are_animations_ongoing());
                    (pos, tile.tile_size())
                };
                let body = pos + smithay::utils::Point::from((size.w / 2., size.h / 2.));
                let under = fixture.niri().contents_under(body);
                assert!(under.surface.is_none());
                assert_eq!(under.window.as_ref().unwrap().0, id);
                assert!(
                    matches!(under.window.unwrap().1, HitType::Spine { edges } if edges.is_empty())
                );
                let edge = fixture
                    .niri()
                    .contents_under(pos + smithay::utils::Point::from((1., 1.)));
                assert!(edge.surface.is_none());
                assert!(
                    matches!(edge.window.unwrap().1, HitType::Spine { edges } if edges == ResizeEdge::TOP_LEFT)
                );
                let outside = match axis {
                    ShrinkAxis::Horizontal => {
                        pos + smithay::utils::Point::from((size.w + 10., size.h / 2.))
                    }
                    ShrinkAxis::Vertical => {
                        pos + smithay::utils::Point::from((size.w / 2., size.h + 10.))
                    }
                };
                assert!(fixture.niri().contents_under(outside).window.is_none());
                let extent = fixture
                    .niri()
                    .global_space
                    .output_geometry(&output)
                    .unwrap()
                    .size;
                let c = fixture.client(client);
                let pointer = c
                    .state
                    .virtual_pointer_manager
                    .as_ref()
                    .unwrap()
                    .create_virtual_pointer(None, &c.qh, ());
                pointer.motion_absolute(
                    1,
                    body.x.round() as u32,
                    body.y.round() as u32,
                    extent.w as u32,
                    extent.h as u32,
                );
                pointer.frame();
                fixture.double_roundtrip(client);
                assert!(fixture
                    .niri()
                    .seat
                    .get_pointer()
                    .unwrap()
                    .current_focus()
                    .is_none());
                assert!(fixture.niri().keyboard_focus.surface().is_none());
                assert_eq!(
                    fixture
                        .client(client)
                        .window(&surface)
                        .configures_received
                        .len(),
                    configure_count + 1
                );
                // The actual client may commit new geometry/title, but its input area stays hidden.
                let window = fixture.client(client).window(&surface);
                window
                    .xdg_toplevel
                    .set_title("Updated hidden title 编辑器".into());
                window.set_size(1500, 1100);
                window.commit();
                fixture.double_roundtrip(client);
                let under = fixture.niri().contents_under(body);
                assert!(under.surface.is_none());
                assert!(matches!(under.window.unwrap().1, HitType::Spine { .. }));
                assert!(fixture.niri().layout.is_spine(&id));
                assert!(fixture.niri().keyboard_focus.surface().is_none());
                let sequence = fixture.niri().output_state[&output].frame_callback_sequence;
                for _ in 0..16 {
                    fixture.double_roundtrip(client);
                }
                let state = &fixture.niri().output_state[&output];
                assert!(matches!(state.redraw_state, RedrawState::Idle));
                assert_eq!(state.frame_callback_sequence, sequence);
                assert!(!fixture.niri().layout.are_animations_ongoing(Some(&output)));
                assert_eq!(fixture.niri().layout.windows().count(), 1);
                assert!(!fixture
                    .client(client)
                    .window(&surface)
                    .configures_received
                    .last()
                    .unwrap()
                    .1
                    .states
                    .contains(&xdg_toplevel::State::Activated));
                let window = fixture.client(client).window(&surface);
                window.xdg_toplevel.destroy();
                window.xdg_surface.destroy();
                window.surface.destroy();
                fixture.double_roundtrip(client);
                assert_eq!(fixture.niri().layout.windows().count(), 0);
                assert!(!fixture.niri().layout.is_spine(&id));
                assert!(fixture.niri().keyboard_focus.surface().is_none());
            }
        }
    }
}

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

fn restore_lifecycle_fixture(
    floating: bool,
    axis: ShrinkAxis,
) -> (
    Fixture,
    super::client::ClientId,
    Vec<wayland_client::protocol::wl_surface::WlSurface>,
    Vec<smithay::desktop::Window>,
) {
    let mut config = niri_config::Config::default();
    config.animations.off = true;
    config.layout.default_column_width = Some(niri_config::PresetSize::Fixed(100));
    if floating {
        config.window_rules = niri_config::Config::parse_mem(
            r#"window-rule {
            open-floating true
            default-column-width { fixed 100; }
            default-window-height { fixed 100; }
        }"#,
        )
        .unwrap()
        .window_rules;
    }
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();
    let mut surfaces = Vec::new();
    let mut ids = Vec::new();
    for _ in 0..2 {
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
            .find(|(_, mapped)| {
                mapped.toplevel().wl_surface().id().protocol_id() == surface.id().protocol_id()
            })
            .unwrap()
            .1
            .window
            .clone();
        surfaces.push(surface);
        ids.push(id);
    }
    {
        let ws = fixture
            .niri()
            .layout
            .workspaces_mut()
            .find(|ws| ws.has_window(&ids[0]))
            .unwrap();
        assert!(ws
            .tiles_mut()
            .find(|tile| tile.window().window == ids[0])
            .unwrap()
            .set_spine(axis, 40.));
        ws.update_window(&ids[0], None);
    }
    let mut config = niri_config::Config::default();
    config.animations.off = true;
    config.layout.default_column_width = Some(niri_config::PresetSize::Fixed(760));
    if floating {
        config.window_rules = niri_config::Config::parse_mem(
            r#"window-rule {
            open-floating true
            default-column-width { fixed 760; }
            default-window-height { fixed 400; }
        }"#,
        )
        .unwrap()
        .window_rules;
    }
    fixture.niri_state().reload_config(Ok(config));
    {
        let ws = fixture
            .niri()
            .layout
            .workspaces_mut()
            .find(|ws| ws.has_window(&ids[0]))
            .unwrap();
        assert_eq!(ws.restore_normal_spines(&ids[..1]), ids[..1]);
        assert!(ws
            .tiles()
            .find(|tile| tile.window().window == ids[0])
            .unwrap()
            .normal_restore_input_blocked());
    }
    fixture.double_roundtrip(client);
    assert_eq!(fixture.niri().layout.focus().unwrap().window, ids[1]);
    (fixture, client, surfaces, ids)
}

#[test]
fn normal_restore_destroy_after_ack_removes_waiter_without_stealing_focus() {
    for floating in [false, true] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            let (mut fixture, client, surfaces, ids) = restore_lifecycle_fixture(floating, axis);
            let (ipc_id, deadline) = fixture
                .niri()
                .layout
                .windows()
                .find(|(_, mapped)| mapped.window == ids[0])
                .map(|(_, mapped)| (mapped.id(), mapped.normal_restore_deadline().unwrap()))
                .unwrap();
            fixture.client(client).window(&surfaces[0]).ack_last();
            fixture.double_roundtrip(client);
            assert_eq!(
                fixture
                    .niri()
                    .layout
                    .windows()
                    .find(|(_, mapped)| mapped.window == ids[0])
                    .unwrap()
                    .1
                    .normal_restore_deadline(),
                Some(deadline)
            );
            let window = fixture.client(client).window(&surfaces[0]);
            window.xdg_toplevel.destroy();
            window.xdg_surface.destroy();
            window.surface.destroy();
            fixture.double_roundtrip(client);
            assert!(fixture
                .niri()
                .layout
                .windows()
                .all(|(_, mapped)| mapped.id() != ipc_id
                    && mapped.normal_restore_deadline().is_none()));
            assert_eq!(fixture.niri().layout.windows().count(), 1);
            assert_eq!(fixture.niri().layout.focus().unwrap().window, ids[1]);
            std::thread::sleep(
                deadline.saturating_duration_since(std::time::Instant::now())
                    + std::time::Duration::from_millis(5),
            );
            fixture.double_roundtrip(client);
            assert_eq!(fixture.niri().layout.windows().count(), 1);
            assert_eq!(fixture.niri().layout.focus().unwrap().window, ids[1]);
            assert_eq!(
                fixture
                    .niri()
                    .keyboard_focus
                    .surface()
                    .map(|surface| surface.id().protocol_id()),
                Some(surfaces[1].id().protocol_id())
            );
        }
    }
}

#[test]
fn normal_restore_output_migration_and_no_outputs_keep_original_deadline_and_identity() {
    use crate::layout::ActivateWindow;
    use smithay::wayland::seat::WaylandFocus as _;

    for floating in [false, true] {
        let (mut fixture, client, surfaces, ids) =
            restore_lifecycle_fixture(floating, ShrinkAxis::Horizontal);
        let target = &ids[0];
        let ipc_id = fixture
            .niri()
            .layout
            .windows()
            .find(|(_, mapped)| mapped.window == *target)
            .unwrap()
            .1
            .id();
        let deadline = fixture
            .niri()
            .layout
            .windows()
            .find(|(_, mapped)| mapped.window == *target)
            .unwrap()
            .1
            .normal_restore_deadline()
            .unwrap();
        let first_size = fixture
            .client(client)
            .window(&surfaces[0])
            .configures_received
            .last()
            .unwrap()
            .1
            .size;
        fixture.client(client).window(&surfaces[0]).ack_last();
        fixture.double_roundtrip(client);
        fixture.add_output(2, (640, 480));
        let out1 = fixture.niri_output(1);
        let out2 = fixture.niri_output(2);
        out2.change_current_state(
            None,
            Some(smithay::utils::Transform::_90),
            Some(smithay::output::Scale::Fractional(1.25)),
            None,
        );
        fixture.niri().layout.update_output_size(&out2);
        fixture
            .niri()
            .layout
            .move_to_output(Some(target), &out2, None, ActivateWindow::No);
        fixture.double_roundtrip(client);
        let mapped = fixture
            .niri()
            .layout
            .find_window_and_output(target.wl_surface().unwrap().as_ref())
            .unwrap();
        assert_eq!(mapped.0.id(), ipc_id);
        assert_eq!(mapped.1, Some(&out2));
        assert_eq!(mapped.0.normal_restore_deadline(), Some(deadline));
        // The old ack must not satisfy a new tiled allocation on the smaller output.
        if !floating {
            let latest = fixture
                .client(client)
                .window(&surfaces[0])
                .configures_received
                .last()
                .unwrap()
                .1
                .size;
            assert_ne!(latest, first_size);
            let window = fixture.client(client).window(&surfaces[0]);
            window.set_size(first_size.0 as u16, first_size.1 as u16);
            window.commit();
            fixture.double_roundtrip(client);
            assert!(!fixture
                .niri()
                .layout
                .windows()
                .find(|(_, mapped)| mapped.window == *target)
                .unwrap()
                .1
                .normal_restore_ready());
        }
        fixture.niri().remove_output(&out1);
        fixture.niri().remove_output(&out2);
        fixture.double_roundtrip(client);
        assert_eq!(fixture.niri().layout.windows().count(), 2);
        assert!(fixture
            .niri()
            .layout
            .windows()
            .all(|(output, _)| output.is_none()));
        assert_eq!(
            fixture
                .niri()
                .layout
                .windows()
                .find(|(_, mapped)| mapped.window == *target)
                .unwrap()
                .1
                .id(),
            ipc_id
        );
        std::thread::sleep(
            deadline.saturating_duration_since(std::time::Instant::now())
                + std::time::Duration::from_millis(5),
        );
        fixture.double_roundtrip(client);
        assert!(fixture
            .niri()
            .layout
            .windows()
            .find(|(_, mapped)| mapped.window == *target)
            .unwrap()
            .1
            .normal_restore_ready());
        fixture.add_output(1, (1280, 720));
        fixture.double_roundtrip(client);
        let window = fixture.client(client).window(&surfaces[0]);
        let size = window.configures_received.last().unwrap().1.size;
        window.set_size(size.0 as u16, size.1 as u16);
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
        let mapped = fixture
            .niri()
            .layout
            .windows()
            .find(|(_, mapped)| mapped.window == *target)
            .unwrap()
            .1;
        assert_eq!(mapped.id(), ipc_id);
        assert!(mapped.normal_restore_deadline().is_none());
        assert_eq!(fixture.niri().layout.windows().count(), 2);
        assert!(fixture
            .niri()
            .layout
            .focus()
            .is_none_or(|mapped| mapped.window != *target));
    }
}

#[test]
fn previous_window_navigation_skips_spines_without_changing_inventory() {
    use std::time::Duration;

    use niri_config::Action;

    for extra_normal in [false, true] {
        let (mut fixture, client, mut surfaces, mut ids) = explicit_mode_fixture();
        if extra_normal {
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
                .find(|(_, mapped)| {
                    mapped.toplevel().wl_surface().id().protocol_id() == surface.id().protocol_id()
                })
                .unwrap()
                .1
                .window
                .clone();
            ids.push(id);
            surfaces.push(surface);
        }
        let current = ids
            .last()
            .filter(|_| extra_normal)
            .unwrap_or(&ids[0])
            .clone();
        fixture.niri().layout.activate_window(&current);
        fixture.double_roundtrip(client);
        // Give books the newest history entries: inventory must retain them,
        // while ordinary navigation must still choose a normal client.
        let niri = fixture.niri();
        niri.mru_apply_keyboard_commit();
        niri.layout.with_windows_mut(|mapped, _| {
            let stamp = if mapped.window == ids[0] {
                100
            } else if mapped.window == current {
                200
            } else {
                300
            };
            mapped.set_focus_timestamp(Duration::from_secs(stamp));
        });
        fixture
            .niri_state()
            .do_action(Action::FocusWindowPrevious, false);
        fixture.double_roundtrip(client);
        assert_eq!(fixture.niri().layout.focus().unwrap().window, ids[0]);
        assert_eq!(
            fixture
                .niri()
                .keyboard_focus
                .surface()
                .map(|surface| surface.id().protocol_id()),
            Some(surfaces[0].id().protocol_id())
        );
        if extra_normal {
            fixture
                .niri_state()
                .do_action(Action::FocusWindowPrevious, false);
            fixture.double_roundtrip(client);
            assert_eq!(fixture.niri().layout.focus().unwrap().window, current);
            assert_eq!(
                fixture
                    .niri()
                    .keyboard_focus
                    .surface()
                    .map(|surface| surface.id().protocol_id()),
                Some(surfaces.last().unwrap().id().protocol_id())
            );
        }
        assert!(fixture.niri().layout.is_spine(&ids[1]));
        assert!(fixture.niri().layout.is_spine(&ids[2]));
        assert_eq!(fixture.niri().layout.windows().count(), ids.len());
        for surface in &surfaces[1..3] {
            assert!(fixture
                .client(client)
                .window(surface)
                .recent_configures()
                .all(|config| !config.states.contains(&xdg_toplevel::State::Activated)));
        }
        // With no normal client left, history navigation retains no client focus.
        let normal_ids = if extra_normal {
            vec![ids[0].clone(), current]
        } else {
            vec![ids[0].clone()]
        };
        for id in normal_ids {
            let ws = fixture
                .niri()
                .layout
                .workspaces_mut()
                .find(|ws| ws.has_window(&id))
                .unwrap();
            assert!(ws
                .tiles_mut()
                .find(|tile| tile.window().window == id)
                .unwrap()
                .set_spine(ShrinkAxis::Horizontal, 40.));
            ws.update_window(&id, None);
        }
        fixture.double_roundtrip(client);
        assert!(fixture.niri().layout.focus().is_none());
        fixture
            .niri_state()
            .do_action(Action::FocusWindowPrevious, false);
        fixture.double_roundtrip(client);
        assert!(fixture.niri().layout.focus().is_none());
        assert_eq!(fixture.niri().layout.windows().count(), ids.len());
        assert!(fixture.niri().keyboard_focus.surface().is_none());
        assert!(ids.iter().all(|id| fixture.niri().layout.is_spine(id)));
    }
}

#[test]
fn moving_book_keeps_client_geometry_when_toggled_to_floating() {
    use niri_config::Action;

    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let (mut fixture, client, surfaces, ids) = explicit_mode_fixture();
        let id = &ids[1];
        {
            let ws = fixture
                .niri()
                .layout
                .workspaces_mut()
                .find(|ws| ws.has_window(id))
                .unwrap();
            let tile = ws
                .tiles_mut()
                .find(|tile| tile.window().window == *id)
                .unwrap();
            assert!(tile.set_spine(axis, 40.));
            ws.update_window(id, None);
        }
        fixture.niri().layout.activate_window(id);
        fixture.double_roundtrip(client);
        let pointer = fixture.niri().seat.get_pointer().unwrap();
        let location = pointer.current_location();
        assert!(!fixture
            .niri_state()
            .move_cursor_to_focused_tile(crate::niri::CenterCoords::BothAlways));
        assert_eq!(pointer.current_location(), location);
        let configured_size = fixture
            .client(client)
            .window(&surfaces[1])
            .configures_received
            .last()
            .unwrap()
            .1
            .size;
        for surface in &surfaces {
            let _ = fixture.client(client).window(surface).recent_configures();
        }
        let ipc_id = fixture
            .niri()
            .layout
            .windows()
            .find(|(_, mapped)| mapped.window == *id)
            .unwrap()
            .1
            .id()
            .get();
        let output = fixture.niri().layout.active_output().unwrap().clone();
        assert!(fixture.niri().layout.interactive_move_begin(
            id.clone(),
            &output,
            (30., 100.).into()
        ));
        assert!(fixture.niri().layout.interactive_move_update(
            id,
            (300., 0.).into(),
            output,
            (330., 100.).into()
        ));
        fixture.double_roundtrip(client);
        // A real hidden buffer commit must not turn the outer geometry into client geometry.
        let window = fixture.client(client).window(&surfaces[1]);
        window.attach_new_buffer();
        window.set_size(1500, 1100);
        window.commit();
        fixture.double_roundtrip(client);
        fixture
            .niri_state()
            .do_action(Action::ToggleWindowFloatingById(ipc_id), false);
        fixture.double_roundtrip(client);
        fixture.niri().layout.interactive_move_end(id);
        fixture.double_roundtrip(client);
        let ws = fixture
            .niri()
            .layout
            .workspaces()
            .find(|(_, _, ws)| ws.has_window(id))
            .unwrap()
            .2;
        assert!(ws.is_floating(id));
        let tile = ws.tiles().find(|tile| tile.window().window == *id).unwrap();
        assert_eq!(tile.form(), WindowForm::Spine(axis));
        let size = tile.tile_size();
        assert_eq!(
            if axis == ShrinkAxis::Horizontal {
                size.w
            } else {
                size.h
            },
            40.
        );
        assert_eq!(tile.window().size(), (1500, 1100).into());
        assert_eq!(fixture.niri().layout.windows().count(), 3);
        assert!(fixture.niri().layout.is_spine(&ids[2]));
        let window = fixture.client(client).window(&surfaces[1]);
        for configure in window.recent_configures() {
            assert_eq!(configure.size, configured_size);
            assert!(!configure.states.contains(&xdg_toplevel::State::Activated));
        }
    }
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
