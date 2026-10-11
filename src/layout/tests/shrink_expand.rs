use super::*;
use crate::layout::floating::FloatingSpace;
use crate::layout::scrolling::ScrollingSpace;
use crate::layout::shrink_expand::{ShrinkAxis, SpineGeometry, WindowForm};

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
