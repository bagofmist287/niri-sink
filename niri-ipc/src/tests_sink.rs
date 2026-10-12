use super::*;
use crate::state::{EventStreamState, EventStreamStatePart};

fn assert_same_window(left: &Window, right: &Window) {
    assert_eq!(
        serde_json::to_value(left).unwrap(),
        serde_json::to_value(right).unwrap()
    );
}

const LEGACY_WINDOW: &str = r#"{
  "id":7,"title":"terminal","app_id":"foot","pid":null,"workspace_id":1,
  "is_focused":false,"is_floating":true,"is_urgent":false,"focus_timestamp":null,
  "layout":{"pos_in_scrolling_layout":null,"tile_size":[400.0,300.0],"window_size":[396,296],
  "tile_pos_in_workspace_view":null,"window_offset_in_tile":[2.0,2.0]}
}"#;
const LEGACY_WORKSPACE: &str = r#"{
  "id":1,"idx":1,"name":null,"output":"output1","is_urgent":false,
  "is_active":true,"is_focused":true,"active_window_id":null
}"#;

#[test]
fn legacy_inventory_defaults_to_normal_owners() {
    let window: Window = serde_json::from_str(LEGACY_WINDOW).unwrap();
    assert!(!window.is_sunk);
    assert_eq!(window.sink_depth, None);
    assert_eq!(window.shrink_state, ShrinkState::Normal);
    assert_eq!(window.shrink_axis, None);
    assert!(window.is_floating);
    let workspace: Workspace = serde_json::from_str(LEGACY_WORKSPACE).unwrap();
    assert_eq!(workspace.sink_count, 0);
}

#[test]
fn sink_inventory_roundtrip_and_existing_events_preserve_identity() {
    let mut state = EventStreamState::default();
    let mut window: Window = serde_json::from_str(LEGACY_WINDOW).unwrap();
    let mut workspace: Workspace = serde_json::from_str(LEGACY_WORKSPACE).unwrap();
    state.apply(Event::WindowsChanged {
        windows: vec![window.clone()],
    });
    state.apply(Event::WorkspacesChanged {
        workspaces: vec![workspace.clone()],
    });
    window.is_sunk = true;
    window.sink_depth = Some(2);
    state.apply(Event::WindowOpenedOrChanged {
        window: window.clone(),
    });
    workspace.sink_count = 3;
    state.apply(Event::WorkspacesChanged {
        workspaces: vec![workspace.clone()],
    });
    assert_eq!(state.windows.windows.len(), 1);
    assert_same_window(&state.windows.windows[&7], &window);
    assert_eq!(state.workspaces.workspaces[&1].sink_count, 3);
    let json = serde_json::to_string(&window).unwrap();
    assert_same_window(&serde_json::from_str::<Window>(&json).unwrap(), &window);
    let mut replica = EventStreamState::default();
    for event in state.replicate() {
        replica.apply(event);
    }
    assert_same_window(&replica.windows.windows[&7], &window);
    assert_eq!(replica.workspaces.workspaces[&1], workspace);
    window.is_sunk = false;
    window.sink_depth = None;
    state.apply(Event::WindowOpenedOrChanged {
        window: window.clone(),
    });
    assert_same_window(&state.windows.windows[&7], &window);
}
