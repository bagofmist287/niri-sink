use crate::{Action, Config};

#[test]
fn shrink_expand_actions_require_explicit_id_and_keep_native_names() {
    let config = Config::parse_mem(
        r#"
binds {
    Mod+S { shrink-window; }
    Mod+E { expand-window id=42; }
    Mod+A { expand-all-windows; }
    Mod+C { confirm-shrink-expand; }
    Mod+X { cancel-shrink-expand; }
}
"#,
    )
    .unwrap();
    for action in [
        Action::ShrinkWindow,
        Action::ExpandWindow(42),
        Action::ExpandAllWindows,
        Action::ConfirmShrinkExpand,
        Action::CancelShrinkExpand,
    ] {
        assert!(config.binds.0.iter().any(|bind| bind.action == action));
    }
    assert!(!config.shrink_expand.enabled);
    for source in [
        "expand-window",
        "expand-window id=-1",
        "shrink-window direction=\"left\"",
        "expand-direct",
    ] {
        assert!(
            Config::parse_mem(&format!("binds {{ Mod+E {{ {source}; }} }}")).is_err(),
            "{source}"
        );
    }
    assert_eq!(
        Action::from(niri_ipc::Action::ExpandWindow { id: 42 }),
        Action::ExpandWindow(42)
    );
}
