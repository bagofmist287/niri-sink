use super::*;
use crate::layout::sink::{RestorePlacement, SinkEntry, SinkSpace};
use crate::layout::workspace::Workspace;

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
