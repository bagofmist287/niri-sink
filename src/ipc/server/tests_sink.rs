use std::collections::HashMap;

use super::*;
use crate::tests::Fixture;

fn drain(events: &Receiver<Event>) -> Vec<Event> {
    let mut out = Vec::new();
    while let Ok(event) = events.try_recv() {
        out.push(event);
    }
    out
}

fn changed_windows(events: &[Event]) -> HashMap<u64, niri_ipc::Window> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::WindowOpenedOrChanged { window } => Some((window.id, window.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn sink_ipc_depth_updates_keep_real_wayland_owners_in_inventory() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();
    let first = fixture.client(client).create_window().surface.clone();
    fixture.client(client).window(&first).commit();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&first);
    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    fixture.niri().layout.toggle_window_floating(None);
    fixture.double_roundtrip(client);
    fixture.client(client).window(&first).ack_last_and_commit();
    fixture.double_roundtrip(client);
    let first_id = fixture
        .niri()
        .layout
        .focus_with_output()
        .unwrap()
        .0
        .id()
        .get();
    let server = fixture.niri().ipc_server.as_ref().unwrap();
    let initial = server.event_stream_state.borrow().windows.windows[&first_id].clone();
    assert!(!initial.is_sunk);
    assert!(initial.is_floating);
    let (send, events) = async_channel::bounded(EVENT_STREAM_BUFFER_SIZE);
    let (disconnect, _disconnect_rx) = async_channel::bounded(1);
    server.event_streams.borrow_mut().push(EventStreamSender {
        events: send,
        disconnect,
    });

    assert!(fixture.niri().layout.sink_window());
    fixture.double_roundtrip(client);
    let first_events = drain(&events);
    assert!(!first_events
        .iter()
        .any(|event| matches!(event, Event::WindowClosed { .. })));
    let sunk = changed_windows(&first_events).remove(&first_id).unwrap();
    assert!(sunk.is_sunk);
    assert_eq!(sunk.sink_depth, Some(0));
    assert_eq!(sunk.layout.window_size, initial.layout.window_size);
    assert!(sunk.is_floating);
    assert!(sunk.layout.pos_in_scrolling_layout.is_none());

    // Native size-state requests receive a real configure without changing Sink ownership.
    // Start after acknowledging Sink's native activation change, excluding mapping history.
    fixture.client(client).window(&first).ack_last_and_commit();
    fixture.double_roundtrip(client);
    let _ = fixture
        .client(client)
        .window(&first)
        .recent_configures()
        .count();
    let window = fixture.client(client).window(&first);
    window.set_fullscreen(None);
    window.set_maximized();
    fixture.double_roundtrip(client);
    let configures = fixture
        .client(client)
        .window(&first)
        .recent_configures()
        .cloned()
        .collect::<Vec<_>>();
    assert!(!configures.is_empty());
    for configure in &configures {
        assert!(configure.states.contains(&smithay::reexports::wayland_protocols::xdg::shell::client::xdg_toplevel::State::Fullscreen));
        assert!(!configure.states.contains(&smithay::reexports::wayland_protocols::xdg::shell::client::xdg_toplevel::State::Maximized));
        assert_eq!(configure.size, (1280, 720));
    }
    let window = fixture.client(client).window(&first);
    window.set_size(1280, 720);
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    let native_events = drain(&events);
    let changes = changed_windows(&native_events);
    assert!(changes[&first_id].is_sunk);
    assert_eq!(changes[&first_id].sink_depth, Some(0));
    assert!(!changes[&first_id].is_floating);
    // Mode metadata may arrive before the commit. Native geometry then uses the existing
    // batched layout event, rather than opening/changing the same owner a second time.
    assert!(native_events.iter().any(|event| matches!(event,
        Event::WindowLayoutsChanged { changes } if changes.iter().any(|(id, layout)|
            *id == first_id && layout.window_size == (1280, 720)))));
    assert_eq!(
        fixture
            .niri()
            .ipc_server
            .as_ref()
            .unwrap()
            .event_stream_state
            .borrow()
            .windows
            .windows[&first_id]
            .layout
            .window_size,
        (1280, 720)
    );
    assert!(fixture.niri().layout.focus().is_none());

    let second = fixture.client(client).create_window().surface.clone();
    fixture.client(client).window(&second).commit();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&second);
    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    let second_window = fixture
        .niri()
        .layout
        .focus_with_output()
        .unwrap()
        .0
        .window
        .clone();
    let second_id = fixture
        .niri()
        .layout
        .focus_with_output()
        .unwrap()
        .0
        .id()
        .get();
    drain(&events);
    assert!(fixture.niri().layout.sink_window());
    fixture.double_roundtrip(client);
    let changes = changed_windows(&drain(&events));
    assert_eq!(changes[&first_id].sink_depth, Some(1));
    assert_eq!(changes[&second_id].sink_depth, Some(0));
    let server = fixture.niri().ipc_server.as_ref().unwrap();
    let state = server.event_stream_state.borrow();
    let ws = &state.workspaces.workspaces[&sunk.workspace_id.unwrap()];
    assert_eq!(ws.sink_count, 2);
    assert_eq!(state.windows.windows.len(), 2);
    drop(state);

    assert_eq!(fixture.niri().layout.pull_window().unwrap(), second_window);
    fixture.double_roundtrip(client);
    let changes = changed_windows(&drain(&events));
    assert_eq!(changes[&first_id].sink_depth, Some(0));
    assert!(!changes[&second_id].is_sunk);
    assert_eq!(changes[&second_id].sink_depth, None);

    // An actual Wayland unmap still emits WindowClosed for that owner, including at depth zero.
    first.attach(None, 0, 0);
    first.commit();
    fixture.double_roundtrip(client);
    assert!(drain(&events)
        .iter()
        .any(|event| matches!(event, Event::WindowClosed { id } if *id == first_id)));
    let server = fixture.niri().ipc_server.as_ref().unwrap();
    let state = server.event_stream_state.borrow();
    assert!(!state.windows.windows.contains_key(&first_id));
    assert!(state.windows.windows.contains_key(&second_id));
    assert_eq!(
        state.workspaces.workspaces[&sunk.workspace_id.unwrap()].sink_count,
        0
    );
}
