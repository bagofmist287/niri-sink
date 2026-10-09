use std::time::{Duration, Instant};

use niri_config::{Action, Config};
use smithay::desktop::Window;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::xdg_activation::{XdgActivationHandler, XdgActivationTokenData};

use super::client::ClientId;
use super::*;
use crate::niri::KeyboardFocus;

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
