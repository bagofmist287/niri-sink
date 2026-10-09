use super::*;
use crate::layout::workspace::{Workspace, WorkspaceAddWindowTarget};

#[test]
fn internal_transfer_preserves_output_membership_and_tile() {
    for is_floating in [false, true] {
        let mut layout = Layout::default();
        Op::AddOutput(1).apply(&mut layout);
        let output = layout.outputs().next().unwrap().clone();
        let mut workspace = Workspace::new(output, Clock::with_time(Duration::ZERO), Rc::default());
        let mut params = TestWindowParams::new(1);
        params.is_floating = is_floating;
        let window = TestWindow::new(params);
        let tile = workspace.make_tile(window.clone());
        workspace.add_tile(
            tile,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            ColumnWidth::Fixed(400.),
            false,
            is_floating,
            None,
        );
        assert_eq!(
            window.0.output_events.take(),
            [("output1".to_owned(), true)]
        );

        let mut removed = workspace.detach_tile(&1, Transaction::new());
        assert_eq!(removed.tile.window().id(), &1);
        assert!(Rc::ptr_eq(&window.0, &removed.tile.window().0));
        assert!(window.0.output_events.borrow().is_empty());
        assert!(!workspace.has_windows());
        assert_eq!(removed.is_floating, is_floating);
        workspace.verify_invariants(None);

        // Internal attachment must not reset this native sizing-state restoration flag.
        removed.tile.restore_to_floating = !is_floating;
        workspace.attach_tile(
            removed.tile,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            removed.width,
            removed.is_full_width,
            removed.is_floating,
            None,
        );
        assert!(window.0.output_events.borrow().is_empty());
        assert_eq!(workspace.active_window().map(LayoutElement::id), Some(&1));
        assert_eq!(workspace.is_floating(&1), is_floating);
        assert_eq!(
            workspace.tiles().next().unwrap().restore_to_floating,
            !is_floating
        );
        workspace.verify_invariants(None);

        let removed = workspace.remove_tile(&1, Transaction::new());
        assert_eq!(
            window.0.output_events.take(),
            [("output1".to_owned(), false)]
        );
        workspace.add_tile(
            removed.tile,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            removed.width,
            removed.is_full_width,
            removed.is_floating,
            None,
        );
        assert_eq!(
            window.0.output_events.take(),
            [("output1".to_owned(), true)]
        );
        assert_eq!(
            workspace.tiles().next().unwrap().restore_to_floating,
            is_floating
        );
        workspace.verify_invariants(None);
    }
}

#[test]
fn external_move_notifies_each_actual_output() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddOutput(2).apply(&mut layout);
    let output1 = layout
        .outputs()
        .find(|output| output.name() == "output1")
        .unwrap()
        .clone();
    let output2 = layout
        .outputs()
        .find(|output| output.name() == "output2")
        .unwrap()
        .clone();
    let clock = Clock::with_time(Duration::ZERO);
    let mut source = Workspace::new(output1, clock.clone(), Rc::default());
    let mut target = Workspace::new(output2, clock, Rc::default());
    let window = TestWindow::new(TestWindowParams::new(1));
    source.add_tile(
        source.make_tile(window.clone()),
        WorkspaceAddWindowTarget::Auto,
        ActivateWindow::Yes,
        ColumnWidth::Fixed(400.),
        false,
        false,
        None,
    );
    window.0.output_events.borrow_mut().clear();
    let removed = source.remove_tile(&1, Transaction::new());
    target.add_tile(
        removed.tile,
        WorkspaceAddWindowTarget::Auto,
        ActivateWindow::Yes,
        removed.width,
        removed.is_full_width,
        removed.is_floating,
        None,
    );
    assert_eq!(
        window.0.output_events.take(),
        [("output1".to_owned(), false), ("output2".to_owned(), true),]
    );
    source.verify_invariants(None);
    target.verify_invariants(None);
}
