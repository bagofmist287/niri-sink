use std::time::{Duration, Instant};

use niri_config::{Action, Config};
use smithay::desktop::Window;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::xdg_activation::{XdgActivationHandler, XdgActivationTokenData};

use super::client::ClientId;
use super::*;
use crate::layout::{LayoutElement as _, SizingMode};
use crate::niri::KeyboardFocus;

// Fixtures do not render. Publish a renderer's visibility decision without changing role or
// exercising hardware scanout, so policy tests can distinguish a visible projection from a
// foreground window.
fn publish_surface_visibility(
    surface: &WlSurface,
    output: &smithay::output::Output,
    visible: bool,
) {
    use std::sync::Mutex;

    use smithay::backend::renderer::element::{
        Id, PrimaryScanoutOutput, RenderElementPresentationState, RenderElementState,
        RenderElementStates,
    };
    use smithay::wayland::compositor::with_states;

    let id = Id::from_wayland_resource(surface);
    let mut rendered = RenderElementStates::default();
    if visible {
        rendered.states.insert(
            id.clone(),
            RenderElementState {
                visible_area: 100,
                presentation_state: RenderElementPresentationState::Rendering { reason: None },
                needs_capture: false,
            },
        );
    }
    with_states(surface, |states| {
        states
            .data_map
            .get_or_insert_threadsafe(Mutex::<PrimaryScanoutOutput>::default)
            .lock()
            .unwrap()
            .update_from_render_element_states(id, output, None, &rendered, |_, _, next, _| next);
    });
}

#[test]
fn idle_policy_excludes_visible_sunk_root_subsurface_and_popup() {
    use std::sync::atomic::Ordering;

    use smithay::desktop::utils::surface_primary_scanout_output;
    use smithay::wayland::compositor::with_states;
    use smithay::wayland::idle_inhibit::IdleInhibitHandler;
    use wayland_client::protocol::wl_subcompositor::WlSubcompositor;

    let mut config = Config::default();
    config.sink.enabled = true;
    let (mut fixture, client, windows) = setup(config);
    fixture.niri_state().do_action(Action::PullWindow, false);
    settle(&mut fixture, client);
    let subcompositor = fixture.client(client).bind::<WlSubcompositor>();
    let state = &mut fixture.client(client).state;
    let parent_surface = state.windows[2].surface.clone();
    let parent_xdg = state.windows[2].xdg_surface.clone();
    let child = state.create_surface();
    let subsurface = subcompositor.get_subsurface(&child, &parent_surface, &state.qh, ());
    child.commit();
    parent_surface.commit();
    let popup_surface = state.create_surface();
    let base = state.xdg_wm_base.as_ref().unwrap();
    let positioner = base.create_positioner(&state.qh, ());
    positioner.set_size(20, 20);
    positioner.set_anchor_rect(0, 0, 10, 10);
    let xdg = base.get_xdg_surface(&popup_surface, &state.qh, ());
    let popup = xdg.get_popup(Some(&parent_xdg), &positioner, &state.qh, ());
    state.popup_surfaces.push(xdg.clone());
    popup_surface.commit();
    fixture.double_roundtrip(client);
    let mut owned = Vec::new();
    windows[2]
        .1
        .with_surfaces(|surface, _| owned.push(surface.clone()));
    assert_eq!(owned.len(), 3);
    let output = fixture.niri_output(1);

    for surface in &owned {
        publish_surface_visibility(surface, &output, true);
        IdleInhibitHandler::inhibit(fixture.niri_state(), surface.clone());
        fixture.niri().refresh_idle_inhibit();
        assert!(fixture.niri().idle_notifier_state.is_inhibited());
        IdleInhibitHandler::uninhibit(fixture.niri_state(), surface.clone());
    }
    fixture.niri_state().do_action(Action::SinkWindow, false);
    assert_eq!(fixture.niri().layout.sink_depth(&windows[2].1), Some(0));
    for surface in &owned {
        publish_surface_visibility(surface, &output, true);
        IdleInhibitHandler::inhibit(fixture.niri_state(), surface.clone());
        fixture.niri().refresh_idle_inhibit();
        assert!(!fixture.niri().idle_notifier_state.is_inhibited());
        assert_eq!(
            with_states(surface, |states| surface_primary_scanout_output(
                surface, states
            )),
            Some(output.clone()),
            "policy filtering must preserve render visibility for callbacks"
        );
        fixture
            .niri()
            .is_fdo_idle_inhibited
            .store(true, Ordering::SeqCst);
        fixture.niri().refresh_idle_inhibit();
        assert!(fixture.niri().idle_notifier_state.is_inhibited());
        fixture
            .niri()
            .is_fdo_idle_inhibited
            .store(false, Ordering::SeqCst);
        IdleInhibitHandler::uninhibit(fixture.niri_state(), surface.clone());
    }
    fixture.niri_state().do_action(Action::PullWindow, false);
    settle(&mut fixture, client);
    for surface in &owned {
        publish_surface_visibility(surface, &output, true);
        IdleInhibitHandler::inhibit(fixture.niri_state(), surface.clone());
        fixture.niri().refresh_idle_inhibit();
        assert!(fixture.niri().idle_notifier_state.is_inhibited());
        publish_surface_visibility(surface, &output, false);
        fixture.niri().refresh_idle_inhibit();
        assert!(!fixture.niri().idle_notifier_state.is_inhibited());
        IdleInhibitHandler::uninhibit(fixture.niri_state(), surface.clone());
    }
    popup.destroy();
    xdg.destroy();
    popup_surface.destroy();
    subsurface.destroy();
    child.destroy();
    fixture.double_roundtrip(client);
}

#[test]
fn vrr_policy_requires_visible_normal_window_on_matching_output() {
    for rule in [false, true] {
        let config = Config::parse_mem(&format!(
            "sink {{ enabled true; }}\nwindow-rule {{ variable-refresh-rate {rule}; }}"
        ))
        .unwrap();
        let (mut fixture, client, windows) = setup(config);
        fixture.add_output(2, (1280, 720));
        let output = fixture.niri_output(1);
        let other_output = fixture.niri_output(2);
        for (surface, _, _) in &windows {
            publish_surface_visibility(surface, &output, true);
        }
        assert!(!fixture.niri().has_visible_vrr_window(&output));
        fixture.niri_state().do_action(Action::PullWindow, false);
        settle(&mut fixture, client);
        publish_surface_visibility(&windows[2].0, &output, true);
        assert_eq!(fixture.niri().has_visible_vrr_window(&output), rule);
        assert!(!fixture.niri().has_visible_vrr_window(&other_output));
        publish_surface_visibility(&windows[2].0, &output, false);
        assert!(!fixture.niri().has_visible_vrr_window(&output));
        publish_surface_visibility(&windows[2].0, &output, true);
        fixture.niri_state().do_action(Action::SinkWindow, false);
        assert!(!fixture.niri().has_visible_vrr_window(&output));
    }
}

fn setup(config: Config) -> (Fixture, ClientId, Vec<(WlSurface, Window, u64)>) {
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();
    let mut windows = Vec::new();
    for _ in 0..3 {
        let surface = fixture.client(client).create_window().surface.clone();
        fixture.client(client).window(&surface).commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.set_size(100, 100);
        window.ack_last_and_commit();
        window.configures_looked_at = window.configures_received.len();
        fixture.double_roundtrip(client);
        let mapped = fixture.niri().layout.focus_with_output().unwrap().0;
        windows.push((
            mapped.window.toplevel().unwrap().wl_surface().clone(),
            mapped.window.clone(),
            mapped.id().get(),
        ));
        assert!(fixture.niri().layout.sink_window());
        fixture.double_roundtrip(client);
    }
    (fixture, client, windows)
}

// Respond to all normal insertion configures, then finish only the visual phase.
fn settle(fixture: &mut Fixture, client: ClientId) {
    for _ in 0..3 {
        fixture.double_roundtrip(client);
        for window in &mut fixture.client(client).state.windows {
            if window.configures_received.len() <= window.configures_looked_at {
                continue;
            }
            if let Some((_, configure)) = window.configures_received.last() {
                window.set_size(
                    configure.size.0.max(100) as u16,
                    configure.size.1.max(100) as u16,
                );
                window.ack_last_and_commit();
                window.configures_looked_at = window.configures_received.len();
            }
        }
    }
    fixture.double_roundtrip(client);
    fixture.niri_complete_animations();
    let off = fixture.niri().config.borrow().animations.off;
    fixture.niri().clock.set_complete_instantly(off);
    fixture.double_roundtrip(client);
}

#[test]
fn explicit_focus_action_unwinds_to_final_client_only() {
    let (mut fixture, client, windows) = setup(Config::default());
    fixture
        .niri_state()
        .do_action(Action::FocusWindow(windows[0].2), false);
    fixture.double_roundtrip(client);
    let niri = fixture.niri();
    for (_, window, _) in &windows {
        assert_eq!(niri.layout.sink_depth(window), None);
    }
    assert_eq!(
        niri.layout.focus_with_output().unwrap().0.window,
        windows[0].1
    );
    assert_eq!(niri.keyboard_focus.surface(), Some(&windows[0].0));
}

#[test]
fn blocked_input_owner_and_dnd_leave_recall_stack_intact() {
    let (mut fixture, _, windows) = setup(Config::default());
    fixture.niri().keyboard_focus = KeyboardFocus::ScreenshotUi;
    fixture
        .niri_state()
        .do_action(Action::FocusWindow(windows[0].2), false);
    assert_eq!(fixture.niri().layout.sink_depth(&windows[0].1), Some(2));
    assert_eq!(fixture.niri().keyboard_focus, KeyboardFocus::ScreenshotUi);
    fixture.niri().keyboard_focus = KeyboardFocus::Layout { surface: None };
    let output = fixture.niri_output(1);
    fixture
        .niri()
        .layout
        .dnd_update(output, (100., 100.).into());
    fixture
        .niri_state()
        .do_action(Action::FocusWindow(windows[0].2), false);
    assert_eq!(fixture.niri().layout.sink_depth(&windows[0].1), Some(2));
    assert!(!fixture.niri().layout.can_transfer_sink());
    fixture.niri().layout.dnd_end();
    assert!(fixture.niri_state().prepare_sink_recall(&windows[0].1));
    assert_eq!(fixture.niri().layout.sink_depth(&windows[0].1), None);
}

#[test]
fn activation_policy_only_focus_recalls_sunk_clients() {
    for policy in ["ignore", "set-urgent", "focus"] {
        let config =
            Config::parse_mem(&format!("window-rule {{ on-xdg-activate \"{policy}\"; }}")).unwrap();
        let (mut fixture, client, windows) = setup(config);
        let (token, data) = {
            let (token, data) = fixture.niri().activation_state.create_external_token(None);
            (token.clone(), data.clone())
        };
        XdgActivationHandler::request_activation(
            fixture.niri_state(),
            token.clone(),
            data,
            windows[0].0.clone(),
        );
        fixture.double_roundtrip(client);
        assert!(fixture
            .niri()
            .activation_state
            .data_for_token(&token)
            .is_none());
        let niri = fixture.niri();
        if policy == "focus" {
            assert_eq!(niri.layout.sink_depth(&windows[0].1), None);
            assert_eq!(
                niri.layout.focus_with_output().unwrap().0.window,
                windows[0].1
            );
        } else {
            assert_eq!(niri.layout.sink_depth(&windows[0].1), Some(2));
            let (_, mapped) = niri
                .layout
                .windows()
                .find(|(_, mapped)| mapped.window == windows[0].1)
                .unwrap();
            assert_eq!(mapped.is_urgent(), policy == "set-urgent");
        }
    }
}

#[test]
fn urgent_only_and_expired_activation_tokens_do_not_unwind() {
    for expired in [false, true] {
        let (mut fixture, client, windows) = setup(Config::default());
        let mut data = XdgActivationTokenData::default();
        if expired {
            data.timestamp = Instant::now() - Duration::from_secs(60);
        }
        let token = fixture
            .niri()
            .activation_state
            .create_external_token(Some(data.clone()))
            .0
            .clone();
        if !expired {
            assert!(XdgActivationHandler::token_created(
                fixture.niri_state(),
                token.clone(),
                data.clone()
            ));
        }
        XdgActivationHandler::request_activation(
            fixture.niri_state(),
            token.clone(),
            data,
            windows[0].0.clone(),
        );
        fixture.double_roundtrip(client);
        assert!(fixture
            .niri()
            .activation_state
            .data_for_token(&token)
            .is_none());
        let niri = fixture.niri();
        assert_eq!(niri.layout.sink_depth(&windows[0].1), Some(2));
        let (_, mapped) = niri
            .layout
            .windows()
            .find(|(_, mapped)| mapped.window == windows[0].1)
            .unwrap();
        assert_eq!(mapped.is_urgent(), !expired);
    }
}

#[test]
fn mru_browsing_is_passive_and_confirmation_recalls_selected_owner() {
    let (mut fixture, client, windows) = setup(Config::default());
    for _ in 0..2 {
        fixture.niri_state().do_action(
            Action::MruAdvance {
                direction: niri_config::MruDirection::Forward,
                scope: None,
                filter: None,
            },
            false,
        );
        fixture.double_roundtrip(client);
        assert!(fixture.niri().window_mru_ui.is_open());
        assert_eq!(
            fixture
                .niri()
                .layout
                .active_workspace()
                .unwrap()
                .sink_count(),
            3
        );
    }
    let selected_id = fixture
        .niri()
        .window_mru_ui
        .current_window_id()
        .unwrap()
        .get();
    let selected = windows
        .iter()
        .find(|(_, _, id)| *id == selected_id)
        .unwrap();
    let depth = fixture.niri().layout.sink_depth(&selected.1).unwrap();
    let timestamps = fixture
        .niri()
        .layout
        .windows()
        .map(|(_, win)| (win.id(), win.get_focus_timestamp()))
        .collect::<Vec<_>>();
    fixture.niri_state().do_action(Action::MruConfirm, false);
    fixture.double_roundtrip(client);
    fixture.niri().mru_apply_keyboard_commit();
    let niri = fixture.niri();
    assert!(!niri.window_mru_ui.is_open());
    assert_eq!(
        niri.layout.active_workspace().unwrap().sink_count(),
        3 - depth - 1
    );
    assert_eq!(niri.layout.sink_depth(&selected.1), None);
    assert_eq!(
        niri.layout.focus_with_output().unwrap().0.window,
        selected.1
    );
    assert_eq!(niri.keyboard_focus.surface(), Some(&selected.0));
    for (_, mapped) in niri.layout.windows() {
        if mapped.id().get() != selected_id {
            let old = timestamps
                .iter()
                .find(|(id, _)| *id == mapped.id())
                .unwrap()
                .1;
            assert_eq!(mapped.get_focus_timestamp(), old);
        }
    }
}

#[test]
fn foreign_activation_checks_recall_input_owner() {
    use crate::protocols::foreign_toplevel::ForeignToplevelHandler;
    let (mut fixture, client, windows) = setup(Config::default());
    fixture.niri().keyboard_focus = KeyboardFocus::ScreenshotUi;
    ForeignToplevelHandler::activate(fixture.niri_state(), windows[0].0.clone());
    assert_eq!(fixture.niri().layout.sink_depth(&windows[0].1), Some(2));
    fixture.niri().keyboard_focus = KeyboardFocus::Layout { surface: None };
    ForeignToplevelHandler::activate(fixture.niri_state(), windows[0].0.clone());
    fixture.double_roundtrip(client);
    assert_eq!(fixture.niri().layout.sink_depth(&windows[0].1), None);
    assert_eq!(
        fixture.niri().layout.focus_with_output().unwrap().0.window,
        windows[0].1
    );
}

#[test]
fn sink_actions_are_opt_in_and_reload_restores_without_stealing_focus() {
    for enabled in [false, true] {
        let mut config = Config::default();
        config.sink.enabled = enabled;
        let (mut fixture, client, windows) = setup(config);
        // setup creates internal owners to also exercise disabling an existing stack.
        fixture.niri_state().do_action(Action::PullWindow, false);
        settle(&mut fixture, client);
        assert_eq!(
            fixture
                .niri()
                .layout
                .active_workspace()
                .unwrap()
                .sink_count(),
            if enabled { 2 } else { 3 }
        );
        if enabled {
            assert_eq!(fixture.niri().layout.focus().unwrap().window, windows[2].1);
            fixture.niri_state().do_action(Action::SinkWindow, false);
            fixture.double_roundtrip(client);
            assert_eq!(fixture.niri().layout.sink_depth(&windows[2].1), Some(0));
            fixture.niri_state().do_action(Action::PullWindow, false);
            settle(&mut fixture, client);
        }
        let before = fixture.niri().layout.focus().map(|win| win.window.clone());
        if !enabled {
            let mut config = Config::default();
            config.sink.enabled = true;
            fixture.niri_state().reload_config(Ok(config));
        }
        fixture.niri_state().reload_config(Ok(Config::default()));
        fixture.double_roundtrip(client);
        assert_eq!(
            fixture
                .niri()
                .layout
                .active_workspace()
                .unwrap()
                .sink_count(),
            0
        );
        assert_eq!(fixture.niri().layout.windows().count(), 3);
        if let Some(before) = before {
            assert_eq!(fixture.niri().layout.focus().unwrap().window, before);
        }
        let count = fixture.niri().layout.windows().count();
        fixture.niri_state().do_action(Action::SinkWindow, false);
        fixture.niri_state().do_action(Action::PullWindow, false);
        assert_eq!(
            fixture
                .niri()
                .layout
                .active_workspace()
                .unwrap()
                .sink_count(),
            0
        );
        assert_eq!(fixture.niri().layout.windows().count(), count);
    }
}

#[test]
fn real_pointer_grab_rejects_transfers_and_recall_without_ending_grab() {
    use crate::input::click_grab::ClickGrab;
    use smithay::input::pointer::{Focus, GrabStartData};
    use smithay::utils::Serial;
    let mut config = Config::default();
    config.sink.enabled = true;
    let (mut fixture, client, windows) = setup(config);
    fixture.niri_state().do_action(Action::PullWindow, false);
    settle(&mut fixture, client);
    let pointer = fixture.niri().seat.get_pointer().unwrap();
    pointer.set_grab(
        fixture.niri_state(),
        ClickGrab::new(GrabStartData {
            focus: None,
            button: 0x110,
            location: (10., 10.).into(),
        }),
        Serial::from(501),
        Focus::Keep,
    );
    for action in [
        Action::SinkWindow,
        Action::PullWindow,
        Action::FocusWindow(windows[0].2),
    ] {
        fixture.niri_state().do_action(action, false);
        assert!(pointer.is_grabbed());
        assert_eq!(fixture.niri().layout.sink_depth(&windows[0].1), Some(1));
        assert_eq!(fixture.niri().layout.focus().unwrap().window, windows[2].1);
    }
    pointer.unset_grab(
        fixture.niri_state(),
        Serial::from(502),
        smithay::backend::input::InputTime::now(),
    );
    fixture.niri_state().do_action(Action::SinkWindow, false);
    assert_eq!(fixture.niri().layout.sink_depth(&windows[2].1), Some(0));
}

#[test]
fn client_issued_activation_token_reaches_policy_and_recalls_only_with_serial() {
    use smithay::reexports::wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;
    for has_serial in [false, true] {
        let mut config = Config::default();
        config.sink.enabled = true;
        let (mut fixture, client, windows) = setup(config);
        fixture.niri_state().do_action(Action::PullWindow, false);
        settle(&mut fixture, client);
        let activation: XdgActivationV1 = fixture.client(client).bind();
        let qh = fixture.client(client).qh.clone();
        let token = activation.get_activation_token(&qh, ());
        if has_serial {
            let state = &fixture.client(client).state;
            let (seat, data) = state.seats.iter().next().unwrap();
            token.set_serial(data.keyboard.as_ref().unwrap().last_enter.unwrap(), seat);
        }
        token.commit();
        fixture.double_roundtrip(client);
        let issued = fixture
            .client(client)
            .state
            .activation_tokens
            .pop()
            .unwrap();
        let surface = fixture.client(client).state.windows[0].surface.clone();
        activation.activate(issued, &surface);
        fixture.double_roundtrip(client);
        assert_eq!(
            fixture.niri().layout.sink_depth(&windows[0].1),
            if has_serial { None } else { Some(1) }
        );
        if has_serial {
            settle(&mut fixture, client);
            assert_eq!(fixture.niri().keyboard_focus.surface(), Some(&windows[0].0));
        }
    }
}

#[test]
fn real_popup_grab_blocks_sink_and_recall_without_dismissing_popup() {
    let mut config = Config::default();
    config.sink.enabled = true;
    let (mut fixture, client, windows) = setup(config);
    fixture.niri_state().do_action(Action::PullWindow, false);
    settle(&mut fixture, client);
    let state = &mut fixture.client(client).state;
    let parent = state.windows[2].xdg_surface.clone();
    let surface = state.create_surface();
    let base = state.xdg_wm_base.as_ref().unwrap();
    let positioner = base.create_positioner(&state.qh, ());
    positioner.set_size(20, 20);
    positioner.set_anchor_rect(0, 0, 10, 10);
    let xdg = base.get_xdg_surface(&surface, &state.qh, ());
    let popup = xdg.get_popup(Some(&parent), &positioner, &state.qh, ());
    let (seat, seat_data) = state.seats.iter().next().unwrap();
    popup.grab(
        seat,
        seat_data.keyboard.as_ref().unwrap().last_enter.unwrap(),
    );
    state.popup_surfaces.push(xdg.clone());
    surface.commit();
    fixture.double_roundtrip(client);
    assert!(fixture.niri().popup_grab.is_some());
    for action in [
        Action::SinkWindow,
        Action::PullWindow,
        Action::FocusWindow(windows[0].2),
    ] {
        fixture.niri_state().do_action(action, false);
        assert!(fixture.niri().popup_grab.is_some());
        assert_eq!(fixture.niri().layout.sink_depth(&windows[0].1), Some(1));
    }
    popup.destroy();
    xdg.destroy();
    surface.destroy();
    fixture.double_roundtrip(client);
}

#[test]
fn actual_session_lock_rejects_even_allow_when_locked_sink_actions() {
    let mut config = Config::default();
    config.sink.enabled = true;
    let (mut fixture, client, windows) = setup(config);
    fixture.niri_state().do_action(Action::PullWindow, false);
    settle(&mut fixture, client);
    let _lock = lock_fixture(&mut fixture, client);
    for action in [
        Action::SinkWindow,
        Action::PullWindow,
        Action::FocusWindow(windows[0].2),
    ] {
        fixture.niri_state().do_action(action, true);
        assert_eq!(fixture.niri().layout.sink_depth(&windows[0].1), Some(1));
        assert_eq!(fixture.niri().layout.sink_depth(&windows[2].1), None);
    }
}

fn resizing_pull() -> (Fixture, ClientId, Vec<(WlSurface, Window, u64)>) {
    let config = Config::parse_mem("sink { enabled true; }; animations { off; }").unwrap();
    let (mut fixture, client, windows) = setup(config);
    fixture.niri_state().do_action(Action::PullWindow, false);
    settle(&mut fixture, client);
    fixture.niri_state().do_action(Action::PullWindow, false);
    settle(&mut fixture, client);
    fixture
        .niri_state()
        .do_action(Action::FocusWindow(windows[2].2), false);
    fixture
        .niri_state()
        .do_action(Action::ConsumeWindowIntoColumn, false);
    settle(&mut fixture, client);
    assert!(fixture.niri().layout.sink_window());
    settle(&mut fixture, client);
    fixture.niri_state().do_action(Action::PullWindow, false);
    fixture.double_roundtrip(client);
    (fixture, client, windows)
}

#[test]
fn pull_ack_only_withholds_keyboard_pointer_and_mru_until_processed_commit() {
    let (mut fixture, client, windows) = resizing_pull();
    let surface = fixture.client(client).state.windows[2].surface.clone();
    assert!(fixture.niri().layout.focus().is_none());
    assert_ne!(fixture.niri().keyboard_focus.surface(), Some(&windows[2].0));
    let before = fixture
        .niri()
        .layout
        .windows()
        .find(|(_, w)| w.window == windows[2].1)
        .unwrap()
        .1
        .get_focus_timestamp();
    fixture.client(client).window(&surface).ack_last();
    fixture.double_roundtrip(client);
    assert!(fixture.niri().layout.focus().is_none());
    let ws = fixture.niri().layout.active_workspace().unwrap();
    let (tile, pos, _) = ws
        .tiles_with_render_positions()
        .find(|(tile, _, _)| tile.window().window == windows[2].1)
        .unwrap();
    assert!(tile.sink_input_blocked());
    assert!(ws
        .window_under(
            pos + tile.window_loc()
                + smithay::utils::Point::<f64, smithay::utils::Logical>::from((10., 10.))
        )
        .is_none());
    assert_eq!(tile.window().get_focus_timestamp(), before);
    // Ack was already sent: commit without acknowledging that same serial again.
    let window = fixture.client(client).window(&surface);
    let size = window.configures_received.last().unwrap().1.size;
    window.set_size(size.0.max(1) as u16, size.1.max(1) as u16);
    window.commit();
    fixture.double_roundtrip(client);
    fixture.niri_complete_animations();
    fixture.double_roundtrip(client);
    assert_eq!(fixture.niri().keyboard_focus.surface(), Some(&windows[2].0));
    assert_eq!(fixture.niri().layout.focus().unwrap().window, windows[2].1);
}

#[test]
fn pull_user_focus_cancels_late_restore_focus_and_fast_reverse_preserves_owner() {
    let (mut fixture, client, windows) = resizing_pull();
    assert!(fixture.niri().layout.focus().is_none());
    assert!(fixture.niri().layout.sink_window());
    assert_eq!(fixture.niri().layout.sink_depth(&windows[2].1), Some(0));
    fixture.niri_state().do_action(Action::PullWindow, false);
    fixture.double_roundtrip(client);
    fixture
        .niri_state()
        .do_action(Action::FocusWindow(windows[0].2), false);
    settle(&mut fixture, client);
    assert_eq!(fixture.niri().layout.focus().unwrap().window, windows[0].1);
    assert_eq!(fixture.niri().layout.windows().count(), 3);
    assert!(fixture
        .niri()
        .layout
        .windows()
        .any(|(_, w)| w.window == windows[2].1));
}

#[test]
fn pull_original_deadline_is_bounded_and_late_commit_cannot_steal_new_focus() {
    let _deadline_guard = crate::utils::transaction::enable_deadline_timers_for_test();
    let (mut fixture, client, windows) = resizing_pull();
    let deadline = fixture
        .niri()
        .layout
        .windows()
        .find(|(_, w)| w.window == windows[2].1)
        .unwrap()
        .1
        .sink_input_deadline()
        .unwrap();
    assert!(fixture.niri().layout.focus().is_none());
    std::thread::sleep(
        deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(15),
    );
    fixture.double_roundtrip(client);
    assert_eq!(fixture.niri().keyboard_focus.surface(), Some(&windows[2].0));
    fixture
        .niri_state()
        .do_action(Action::FocusWindow(windows[1].2), false);
    fixture.double_roundtrip(client);
    settle(&mut fixture, client);
    assert_eq!(fixture.niri().keyboard_focus.surface(), Some(&windows[1].0));
    assert_eq!(fixture.niri().layout.focus().unwrap().window, windows[1].1);
}

#[test]
fn pull_matching_committed_native_size_needs_no_new_resize_commit() {
    let config = Config::parse_mem("sink { enabled true; }; animations { off; }").unwrap();
    let (mut fixture, client, windows) = setup(config);
    fixture.niri_state().do_action(Action::PullWindow, false);
    fixture.double_roundtrip(client);
    let mapped = fixture
        .niri()
        .layout
        .windows()
        .find(|(_, w)| w.window == windows[2].1)
        .unwrap()
        .1;
    assert!(mapped.sink_input_deadline().is_none());
    assert_eq!(fixture.niri().keyboard_focus.surface(), Some(&windows[2].0));
}

#[test]
fn sunk_native_fullscreen_and_maximize_requests_reply_without_recalling() {
    let config = Config::parse_mem("sink { enabled true; }; animations { off; }").unwrap();
    let (mut fixture, client, windows) = setup(config);
    let surface = fixture.client(client).state.windows[2].surface.clone();
    fixture.client(client).window(&surface).set_maximized();
    fixture.double_roundtrip(client);
    settle(&mut fixture, client);
    fixture.client(client).window(&surface).set_fullscreen(None);
    fixture.double_roundtrip(client);
    settle(&mut fixture, client);
    for mode in [SizingMode::Maximized, SizingMode::Normal] {
        let before = fixture
            .client(client)
            .window(&surface)
            .configures_received
            .len();
        if mode.is_maximized() {
            fixture.client(client).window(&surface).unset_fullscreen();
        } else {
            fixture.client(client).window(&surface).unset_maximized();
        }
        fixture.double_roundtrip(client);
        assert!(
            fixture
                .client(client)
                .window(&surface)
                .configures_received
                .len()
                > before
        );
        settle(&mut fixture, client);
        let mapped = fixture
            .niri()
            .layout
            .windows()
            .find(|(_, w)| w.window == windows[2].1)
            .unwrap()
            .1;
        assert_eq!(mapped.sizing_mode(), mode);
        assert_eq!(fixture.niri().layout.sink_depth(&windows[2].1), Some(0));
        assert!(fixture.niri().layout.focus().is_none());
        assert_ne!(fixture.niri().keyboard_focus.surface(), Some(&windows[2].0));
        assert_eq!(fixture.niri().layout.windows().count(), 3);
    }
}

#[test]
fn pull_same_size_fullscreen_state_still_waits_for_processed_commit() {
    let config = Config::parse_mem("sink { enabled true; }; animations { off; }").unwrap();
    let (mut fixture, client, windows) = setup(config);
    let surface = fixture.client(client).state.windows[2].surface.clone();
    let window = fixture.client(client).window(&surface);
    window.set_size(1280, 720);
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    fixture.client(client).window(&surface).set_fullscreen(None);
    fixture.double_roundtrip(client);
    fixture.niri_state().do_action(Action::PullWindow, false);
    fixture.double_roundtrip(client);
    fixture.niri_complete_animations();
    fixture.double_roundtrip(client);
    let mapped = fixture
        .niri()
        .layout
        .windows()
        .find(|(_, w)| w.window == windows[2].1)
        .unwrap()
        .1;
    assert_eq!(mapped.size(), (1280, 720).into());
    assert_eq!(mapped.requested_size(), Some((1280, 720).into()));
    assert_eq!(mapped.sizing_mode(), SizingMode::Normal);
    assert_eq!(mapped.pending_sizing_mode(), SizingMode::Fullscreen);
    assert!(!mapped.sink_input_ready());
    fixture.client(client).window(&surface).ack_last();
    fixture.double_roundtrip(client);
    assert!(fixture.niri().layout.focus().is_none());
    fixture.client(client).window(&surface).commit();
    fixture.double_roundtrip(client);
    assert_eq!(fixture.niri().keyboard_focus.surface(), Some(&windows[2].0));
    assert_eq!(
        fixture.niri().layout.focus().unwrap().sizing_mode(),
        SizingMode::Fullscreen
    );
    assert_eq!(fixture.niri().layout.windows().count(), 3);
}

#[test]
fn pull_new_native_state_retargets_commit_without_extending_deadline() {
    let (mut fixture, client, windows) = resizing_pull();
    let surface = fixture.client(client).state.windows[2].surface.clone();
    let (old_serial, old_configure) = fixture
        .client(client)
        .window(&surface)
        .configures_received
        .last()
        .unwrap()
        .clone();
    let original_deadline = fixture
        .niri()
        .layout
        .windows()
        .find(|(_, w)| w.window == windows[2].1)
        .unwrap()
        .1
        .sink_input_deadline()
        .unwrap();
    fixture.client(client).window(&surface).set_fullscreen(None);
    fixture.double_roundtrip(client);
    assert_eq!(
        fixture
            .niri()
            .layout
            .windows()
            .find(|(_, w)| w.window == windows[2].1)
            .unwrap()
            .1
            .sink_input_deadline(),
        Some(original_deadline)
    );
    let window = fixture.client(client).window(&surface);
    window.xdg_surface.ack_configure(old_serial);
    window.set_size(
        old_configure.size.0.max(1) as u16,
        old_configure.size.1.max(1) as u16,
    );
    window.commit();
    fixture.double_roundtrip(client);
    fixture.niri_complete_animations();
    fixture.double_roundtrip(client);
    assert!(fixture.niri().layout.focus().is_none());
    let window = fixture.client(client).window(&surface);
    let size = window.configures_received.last().unwrap().1.size;
    window.set_size(size.0.max(1) as u16, size.1.max(1) as u16);
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    assert_eq!(fixture.niri().keyboard_focus.surface(), Some(&windows[2].0));
    assert_eq!(
        fixture.niri().layout.focus().unwrap().sizing_mode(),
        SizingMode::Fullscreen
    );
}

#[test]
fn pull_unmap_and_feature_reload_invalidate_pending_focus() {
    for unmap in [true, false] {
        let (mut fixture, client, windows) = resizing_pull();
        assert!(fixture.niri().layout.focus().is_none());
        let surface = fixture.client(client).state.windows[2].surface.clone();
        if unmap {
            let window = fixture.client(client).window(&surface);
            window.attach_null();
            window.commit();
        } else {
            fixture
                .niri_state()
                .do_action(Action::FocusWindow(windows[1].2), false);
            fixture.niri_state().reload_config(Ok(Config::default()));
        }
        fixture.double_roundtrip(client);
        if !unmap {
            settle(&mut fixture, client);
        }
        fixture.niri_complete_animations();
        fixture.double_roundtrip(client);
        assert_ne!(fixture.niri().keyboard_focus.surface(), Some(&windows[2].0));
        assert_eq!(
            fixture.niri().layout.windows().count(),
            if unmap { 2 } else { 3 }
        );
    }
}

#[test]
fn pull_new_input_owner_cancels_focus_even_after_matching_commit() {
    let (mut fixture, client, windows) = resizing_pull();
    fixture.niri().keyboard_focus = KeyboardFocus::ScreenshotUi;
    fixture.double_roundtrip(client);
    settle(&mut fixture, client);
    assert_ne!(fixture.niri().keyboard_focus.surface(), Some(&windows[2].0));
    // Ending the other owner's interaction does not revive the old restore plan.
    fixture.niri().keyboard_focus = KeyboardFocus::Layout { surface: None };
    fixture.double_roundtrip(client);
    assert!(fixture.niri().layout.focus().is_none());
    fixture
        .niri_state()
        .do_action(Action::FocusColumnLast, false);
    fixture.double_roundtrip(client);
    assert_eq!(fixture.niri().keyboard_focus.surface(), Some(&windows[2].0));
}

fn lock_fixture(fixture: &mut Fixture, client: ClientId) -> smithay::reexports::wayland_protocols::ext::session_lock::v1::client::ext_session_lock_v1::ExtSessionLockV1{
    use smithay::reexports::wayland_protocols::ext::session_lock::v1::client::ext_session_lock_manager_v1::ExtSessionLockManagerV1;
    let manager: ExtSessionLockManagerV1 = fixture.client(client).bind();
    let qh = fixture.client(client).qh.clone();
    let lock = manager.lock(&qh, ());
    let output = fixture.client(client).output("headless-1");
    let state = &mut fixture.client(client).state;
    let surface = state.create_surface();
    let _lock_surface = lock.get_lock_surface(&surface, &output, &qh, ());
    fixture.double_roundtrip(client);
    let state = &fixture.client(client).state;
    let buffer =
        state
            .spbm
            .as_ref()
            .unwrap()
            .create_u32_rgba_buffer(u32::MAX, 0, 0, u32::MAX, &qh, ());
    let viewport = state
        .viewporter
        .as_ref()
        .unwrap()
        .get_viewport(&surface, &qh, ());
    viewport.set_destination(1280, 720);
    surface.attach(Some(&buffer), 0, 0);
    surface.commit();
    fixture.double_roundtrip(client);
    assert!(fixture.niri().is_locked());
    lock
}

#[test]
fn pull_session_lock_cancels_old_restore_through_commit_and_unlock() {
    let (mut fixture, client, windows) = resizing_pull();
    let lock = lock_fixture(&mut fixture, client);
    settle(&mut fixture, client);
    assert_ne!(fixture.niri().keyboard_focus.surface(), Some(&windows[2].0));
    lock.unlock_and_destroy();
    fixture.double_roundtrip(client);
    assert!(!fixture.niri().is_locked());
    assert_ne!(fixture.niri().keyboard_focus.surface(), Some(&windows[2].0));
    fixture
        .niri_state()
        .do_action(Action::FocusColumnLast, false);
    fixture.double_roundtrip(client);
    assert_eq!(fixture.niri().keyboard_focus.surface(), Some(&windows[2].0));
}
