use super::*;
use crate::layout::floating::FloatingSpace;
use crate::layout::scrolling::ScrollingSpace;
use crate::layout::shrink_expand::{ShrinkAxis, SpineGeometry, WindowForm};
use crate::layout::workspace::{Workspace, WorkspaceAddWindowTarget};

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
fn current_slot_normal_plan_uses_live_defaults_rules_and_native_insertion_width() {
    for rule_width in [None, Some(Some(PresetSize::Fixed(850))), Some(None)] {
        let mut options = Options::default();
        options.layout.default_column_width = Some(PresetSize::Fixed(720));
        let mut workspace =
            Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::new(options.clone()));
        let mut params = TestWindowParams::new(1);
        params.bbox = Rectangle::from_size((900, 300).into());
        params.rules = Some(crate::window::ResolvedWindowRules {
            default_width: rule_width,
            ..Default::default()
        });
        let client = TestWindow::new(params);
        let mut book = workspace.make_tile(client.clone());
        assert!(book.set_spine(ShrinkAxis::Horizontal, 40.));
        workspace.add_tile(
            book,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            ColumnWidth::Fixed(1100.),
            true,
            false,
            None,
        );
        // Both the current configuration and the client's own size can change while folded.
        options.layout.default_column_width = Some(PresetSize::Fixed(760));
        workspace.update_config(Rc::new(options.clone()));
        client.0.bbox.set(Rectangle::from_size((680, 300).into()));
        workspace.update_window(&1, None);
        let normal = workspace.make_tile(TestWindow::new(TestWindowParams::new(2)));
        workspace.add_tile(
            normal,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            ColumnWidth::Fixed(400.),
            false,
            false,
            None,
        );
        let sizes = workspace.tiles().map(|t| t.tile_size()).collect::<Vec<_>>();
        let focus = workspace.selected_window().map(|w| *w.id());
        let plans = workspace.normal_spine_plans(&[99, 1, 2, 1]);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].0, 1);
        assert_eq!(
            workspace.tiles().map(|t| t.tile_size()).collect::<Vec<_>>(),
            sizes
        );
        assert_eq!(workspace.selected_window().map(|w| *w.id()), focus);
        assert_eq!(client.requested_size(), None);
        assert!(client.0.output_events.borrow().is_empty());
        let width = match rule_width {
            None => 760.,
            Some(Some(_)) => 850.,
            Some(None) => 680.,
        };
        let mut oracle =
            Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::new(options));
        let ordinary = TestWindow::new(TestWindowParams::new(1));
        let tile = oracle.make_tile(ordinary.clone());
        oracle.add_tile(
            tile,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            ColumnWidth::Fixed(width),
            false,
            false,
            None,
        );
        assert_eq!(plans[0].1.client_size, ordinary.requested_size().unwrap());
        assert_eq!(plans[0].1.sizing_mode, SizingMode::Normal);
        assert_ne!(plans[0].1.client_size.w, 1100);
        assert_eq!(workspace.windows().count(), 2);
        assert!(workspace
            .tiles()
            .find(|t| t.window().id() == &1)
            .unwrap()
            .is_spine());
    }
}

#[test]
fn batch_normal_plans_replace_all_selected_constraints_in_one_column_snapshot() {
    for scale in [1., 1.25] {
        for display in [ColumnDisplay::Normal, ColumnDisplay::Tabbed] {
            for fixed_height in [false, true] {
                let mut options = Options::default();
                options.layout.default_column_display = display;
                options.layout.border.off = false;
                options.layout.border.width = 1.3;
                let options = Rc::new(options);
                let mut space = scrolling(scale, options.clone());
                let mut clients = Vec::new();
                for id in 1..=3 {
                    let mut params = TestWindowParams::new(id);
                    params.min_max_size = ((100, 20).into(), (0, 0).into());
                    let client = TestWindow::new(params);
                    let mut tile = Tile::new(
                        client.clone(),
                        space.view_size(),
                        scale,
                        space.clock().clone(),
                        options.clone(),
                    );
                    if id > 1 {
                        assert!(tile.set_spine(ShrinkAxis::Vertical, 40.));
                    }
                    if id == 1 {
                        space.add_tile(None, tile, true, ColumnWidth::Fixed(700.), false, None);
                    } else {
                        space.add_tile_to_column(0, None, tile, false);
                    }
                    clients.push(client);
                }
                settle(&mut space);
                if fixed_height {
                    space.set_window_height(Some(&2), SizeChange::SetFixed(120));
                }
                let requests_before = clients
                    .iter()
                    .map(TestWindow::requested_size)
                    .collect::<Vec<_>>();
                let sizes_before = space.tiles().map(|t| t.tile_size()).collect::<Vec<_>>();
                let plans = space.normal_spine_plans(&[
                    (3, ColumnWidth::Fixed(1200.)),
                    (1, ColumnWidth::Fixed(1200.)),
                    (2, ColumnWidth::Fixed(1200.)),
                    (2, ColumnWidth::Fixed(100.)),
                ]);
                assert_eq!(plans.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [2, 3]);
                assert_eq!(
                    clients
                        .iter()
                        .map(TestWindow::requested_size)
                        .collect::<Vec<_>>(),
                    requests_before
                );
                assert_eq!(
                    space.tiles().map(|t| t.tile_size()).collect::<Vec<_>>(),
                    sizes_before
                );
                let single = space.normal_spine_plans(&[(2, ColumnWidth::Fixed(1200.))]);
                if display == ColumnDisplay::Normal && !fixed_height {
                    assert_ne!(single[0].1.allocation.h, plans[0].1.allocation.h);
                }
                // The ordinary allocator is the oracle, with the same members and native Auto(1)
                // insertion height. Clearing form here is a test setup, not a user Expand action.
                for tile in space.tiles_mut().filter(|t| t.is_spine()) {
                    tile.clear_spine();
                }
                space.reset_window_height(Some(&2));
                space.reset_window_height(Some(&3));
                settle(&mut space);
                for (id, plan) in plans {
                    assert_eq!(plan.client_size, clients[id - 1].requested_size().unwrap());
                    assert_eq!(plan.sizing_mode, SizingMode::Normal);
                }
                assert_eq!(space.columns().count(), 1);
                assert_eq!(
                    space.tiles().map(|t| *t.window().id()).collect::<Vec<_>>(),
                    [1, 2, 3]
                );
            }
        }
    }
}

#[test]
fn initial_spine_geometry_rejects_pending_and_committed_windowed_fullscreen() {
    let mut tile = tile(1, 1., Rc::default());
    let client = tile.window().clone();
    client.0.is_pending_windowed_fullscreen.set(true);
    assert!(tile
        .plan_spine_geometry(ShrinkAxis::Horizontal, 40.)
        .is_none());
    assert!(!tile.set_spine(ShrinkAxis::Horizontal, 40.));
    assert_eq!(tile.form(), WindowForm::Normal);
    client.communicate();
    tile.update_window();
    client.0.is_pending_windowed_fullscreen.set(false);
    assert!(tile
        .plan_spine_geometry(ShrinkAxis::Vertical, 40.)
        .is_none());
    client.communicate();
    tile.update_window();
    assert!(tile
        .plan_spine_geometry(ShrinkAxis::Vertical, 40.)
        .is_some());
}

#[test]
fn normal_slot_plans_in_non_normal_tabs_inherit_native_container_request() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let mut space = non_normal_space(1.25, mode, ColumnDisplay::Tabbed);
        let options = space.tiles().next().unwrap().options().clone();
        let mut book = scrolling_tile(2, &space, options);
        let client = book.window().clone();
        assert!(book.set_spine(ShrinkAxis::Vertical, 40.));
        space.add_tile_to_column(0, None, book, true);
        let before = space.tiles().map(|t| t.tile_size()).collect::<Vec<_>>();
        let plans = space.normal_spine_plans(&[(2, ColumnWidth::Fixed(800.))]);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].1.sizing_mode, mode);
        assert_eq!(
            plans[0].1.client_size,
            space
                .tiles()
                .next()
                .unwrap()
                .window()
                .requested_size()
                .unwrap()
        );
        assert_eq!(
            plans[0].1.allocation,
            if mode.is_fullscreen() {
                space.view_size()
            } else {
                space.parent_area().size
            }
        );
        assert_eq!(client.requested_size(), None);
        assert_eq!(client.pending_sizing_mode(), SizingMode::Normal);
        assert_eq!(
            space.tiles().map(|t| t.tile_size()).collect::<Vec<_>>(),
            before
        );
        settle(&mut space);
    }
}

#[test]
fn floating_normal_plans_use_native_defaults_without_size_memory_or_recentering() {
    for explicit_size in [false, true] {
        let mut options = Options::default();
        options.layout.default_column_width = Some(PresetSize::Fixed(1000));
        let options = Rc::new(options);
        let mut params = TestWindowParams::new(1);
        params.bbox = Rectangle::from_size((240, 160).into());
        if explicit_size {
            params.min_max_size = ((600, 500).into(), (800, 600).into());
            params.rules = Some(crate::window::ResolvedWindowRules {
                default_width: Some(Some(PresetSize::Fixed(720))),
                default_height: Some(Some(PresetSize::Fixed(400))),
                ..Default::default()
            });
        }
        let client = TestWindow::new(params);
        let clock = Clock::with_time(Duration::ZERO);
        let mut book = Tile::new(
            client.clone(),
            (1280., 720.).into(),
            1.25,
            clock.clone(),
            options.clone(),
        );
        book.floating_window_size = Some((999, 888).into());
        book.floating_pos = Some((0.25, 0.75).into());
        assert!(book.set_spine(ShrinkAxis::Horizontal, 40.));
        let mut space = FloatingSpace::new(
            (1280., 720.).into(),
            Rectangle::from_size((1280., 720.).into()),
            1.25,
            clock,
            options,
        );
        space.add_tile(book, true);
        let before = space
            .tiles_with_offsets()
            .map(|(tile, pos)| (tile.tile_size(), pos))
            .collect::<Vec<_>>();
        let plan = space.normal_spine_plans(&[99, 1, 1]);
        assert_eq!(plan.len(), 1);
        assert_eq!(
            plan[0].1.client_size,
            if explicit_size {
                (720, 500).into()
            } else {
                (0, 0).into()
            }
        );
        assert_eq!(
            plan[0].1.allocation,
            if explicit_size {
                (720., 500.).into()
            } else {
                (240., 160.).into()
            }
        );
        assert_eq!(plan[0].1.sizing_mode, SizingMode::Normal);
        assert_eq!(client.requested_size(), None);
        assert_eq!(
            space.tiles().next().unwrap().floating_window_size,
            Some((999, 888).into())
        );
        assert_eq!(
            space
                .tiles_with_offsets()
                .map(|(tile, pos)| (tile.tile_size(), pos))
                .collect::<Vec<_>>(),
            before
        );
        space.verify_invariants();
    }
}

fn non_normal_space(
    scale: f64,
    mode: SizingMode,
    display: ColumnDisplay,
) -> ScrollingSpace<TestWindow> {
    let mut options = Options::default();
    options.layout.default_column_display = display;
    options.layout.struts.top = niri_config::FloatOrInt(13.);
    options.layout.struts.bottom = niri_config::FloatOrInt(11.);
    let options = Rc::new(options);
    let mut space = ScrollingSpace::new(
        (1280., 720.).into(),
        Rectangle::new((0., 24.).into(), (1280., 664.).into()),
        scale,
        Clock::with_time(Duration::ZERO),
        options.clone(),
    );
    let first = scrolling_tile(1, &space, options);
    space.add_tile(None, first, true, ColumnWidth::Fixed(800.), false, None);
    match mode {
        SizingMode::Fullscreen => assert!(space.set_fullscreen(&1, true)),
        SizingMode::Maximized => assert!(space.set_maximized(&1, true)),
        SizingMode::Normal => unreachable!(),
    }
    settle(&mut space);
    space
}

#[test]
fn passive_spine_insertion_into_fullscreen_or_maximized_tabs_keeps_client_normal() {
    for scale in [1., 1.25] {
        for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
            for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
                for activate in [false, true] {
                    let mut space = non_normal_space(scale, mode, ColumnDisplay::Tabbed);
                    let normal = space.tiles().next().unwrap().window().clone();
                    let normal_request = normal.requested_size();
                    let options = space.tiles().next().unwrap().options().clone();
                    let mut incoming = scrolling_tile(2, &space, options);
                    let client = incoming.window().clone();
                    assert!(incoming.set_spine(axis, 40.1));
                    let client_size = client.size();
                    let plans = space.spine_in_column_plans(0, 0, &incoming, 40.1).unwrap();
                    assert_eq!(client.requested_size(), None);
                    assert_eq!(normal.requested_size(), normal_request);
                    assert!(plans.get(axis).fits);
                    assert_eq!(plans.for_move(axis).geometry.axis, axis);
                    let extent = if mode.is_fullscreen() { 720. } else { 664. };
                    let thickness = (40.1_f64 * scale).ceil() / scale;
                    let expected = match axis {
                        ShrinkAxis::Horizontal => (thickness, extent),
                        ShrinkAxis::Vertical => (1280., thickness),
                    };
                    assert_eq!(plans.get(axis).geometry.outer_size(), expected.into());
                    space.add_tile_to_column(0, Some(0), incoming, activate);
                    settle(&mut space);
                    assert_eq!(space.columns().count(), 1);
                    assert_eq!(
                        space.tiles().map(|t| *t.window().id()).collect::<Vec<_>>(),
                        [2, 1]
                    );
                    let col = space.columns().next().unwrap();
                    assert_eq!(col.pending_sizing_mode(), mode);
                    assert_eq!(
                        space.active_window().unwrap().id(),
                        if activate { &2 } else { &1 }
                    );
                    let book = space.tiles().find(|t| t.window().id() == &2).unwrap();
                    assert_eq!(book.form(), WindowForm::Spine(axis));
                    assert_eq!(book.tile_size(), expected.into());
                    assert_eq!(book.tile_size(), plans.get(axis).geometry.outer_size());
                    assert_eq!(book.sizing_mode(), SizingMode::Normal);
                    assert_eq!(client.pending_sizing_mode(), SizingMode::Normal);
                    assert_eq!(client.sizing_mode(), SizingMode::Normal);
                    assert_eq!(client.requested_size(), None);
                    assert_eq!(client.size(), client_size);
                    assert_eq!(normal.pending_sizing_mode(), mode);
                    assert_eq!(normal.requested_size(), normal_request);
                    assert!(space.spine_plans(&1, 40.1).is_none());
                    assert_eq!(
                        space
                            .spine_plans(&2, 40.1)
                            .unwrap()
                            .get(axis)
                            .geometry
                            .outer_size(),
                        book.tile_size()
                    );
                }
            }
        }
    }
}

#[test]
fn closing_last_normal_tab_keeps_spine_container_origin_until_native_mode_changes() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            let mut space = non_normal_space(1., mode, ColumnDisplay::Tabbed);
            let options = space.tiles().next().unwrap().options().clone();
            let mut incoming = scrolling_tile(2, &space, options.clone());
            let client = incoming.window().clone();
            assert!(incoming.set_spine(axis, 40.));
            space.add_tile_to_column(0, Some(1), incoming, true);
            settle(&mut space);
            let before = space
                .tiles()
                .find(|t| t.window().id() == &2)
                .unwrap()
                .tile_size();
            let removed = space.remove_tile(&1, Transaction::new());
            assert_eq!(removed.tile.window().id(), &1);
            settle(&mut space);
            assert_eq!(space.columns().count(), 1);
            assert_eq!(space.tiles().count(), 1);
            let col = space.columns().next().unwrap();
            let (book, pos) = col.tiles().next().unwrap();
            assert_eq!(book.tile_size(), before);
            assert_eq!(pos.y, if mode.is_fullscreen() { 0. } else { 24. });
            assert_eq!(col.pending_sizing_mode(), mode);
            // Output/config changes allocate the spine immediately, without a hidden commit.
            space.update_config(
                (1000., 600.).into(),
                Rectangle::new((0., 20.).into(), (1000., 550.).into()),
                1.25,
                options,
            );
            settle(&mut space);
            let expected = match axis {
                ShrinkAxis::Horizontal => (40., if mode.is_fullscreen() { 600. } else { 550.4 }),
                ShrinkAxis::Vertical => (1000., 40.),
            };
            assert_eq!(space.tiles().next().unwrap().tile_size(), expected.into());
            client.0.bbox.set(Rectangle::from_size((700, 500).into()));
            space.update_window(&2, None);
            assert_eq!(space.tiles().next().unwrap().tile_size(), expected.into());
            assert_eq!(client.pending_sizing_mode(), SizingMode::Normal);
            assert_eq!(client.requested_size(), None);
            // This internal mode removal must keep the spine folded (MoveGrab uses it too).
            if mode.is_fullscreen() {
                assert!(space.set_fullscreen(&2, false));
            } else {
                assert!(space.set_maximized(&2, false));
            }
            settle(&mut space);
            assert_eq!(
                space.tiles().next().unwrap().form(),
                WindowForm::Spine(axis)
            );
            assert_eq!(
                space.columns().next().unwrap().pending_sizing_mode(),
                SizingMode::Normal
            );
            assert_eq!(client.requested_size(), None);
        }
    }
}

#[test]
fn spine_insertion_into_non_tabbed_fullscreen_column_uses_native_mode_removal() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            let mut space = non_normal_space(1., mode, ColumnDisplay::Normal);
            let normal = space.tiles().next().unwrap().window().clone();
            let options = space.tiles().next().unwrap().options().clone();
            let mut incoming = scrolling_tile(2, &space, options);
            let client = incoming.window().clone();
            assert!(incoming.set_spine(axis, 40.));
            let plans = space.spine_in_column_plans(0, 1, &incoming, 40.).unwrap();
            space.add_tile_to_column(0, Some(1), incoming, true);
            assert_eq!(
                space
                    .tiles()
                    .find(|t| t.window().id() == &2)
                    .unwrap()
                    .tile_size(),
                plans.get(axis).geometry.outer_size()
            );
            settle(&mut space);
            assert_eq!(space.columns().count(), 1);
            assert_eq!(space.tiles().count(), 2);
            assert_eq!(
                space.columns().next().unwrap().pending_sizing_mode(),
                SizingMode::Normal
            );
            assert_eq!(normal.pending_sizing_mode(), SizingMode::Normal);
            assert_eq!(client.pending_sizing_mode(), SizingMode::Normal);
            assert_eq!(client.requested_size(), None);
            assert_eq!(
                space
                    .tiles()
                    .find(|t| t.window().id() == &2)
                    .unwrap()
                    .form(),
                WindowForm::Spine(axis)
            );
        }
    }
}

#[test]
fn spine_only_non_normal_tabs_accept_normal_clients_before_their_first_commit() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            let mut space = non_normal_space(1., mode, ColumnDisplay::Tabbed);
            let options = space.tiles().next().unwrap().options().clone();
            let mut book = scrolling_tile(2, &space, options.clone());
            let hidden = book.window().clone();
            assert!(book.set_spine(axis, 40.));
            space.add_tile_to_column(0, None, book, true);
            space.remove_tile(&1, Transaction::new());
            space.verify_invariants();
            let size = space.tiles().next().unwrap().tile_size();
            let incoming = scrolling_tile(3, &space, options);
            let client = incoming.window().clone();
            space.add_tile_to_column(0, Some(0), incoming, true);
            assert_eq!(client.sizing_mode(), SizingMode::Normal);
            assert_eq!(client.pending_sizing_mode(), mode);
            space.verify_invariants();
            assert_eq!(
                space
                    .tiles()
                    .find(|t| t.window().id() == &2)
                    .unwrap()
                    .tile_size(),
                size
            );
            settle(&mut space);
            assert_eq!(client.sizing_mode(), mode);
            assert_eq!(hidden.pending_sizing_mode(), SizingMode::Normal);
            assert_eq!(hidden.requested_size(), None);
            assert_eq!(space.columns().count(), 1);
            assert_eq!(
                space.tiles().map(|t| *t.window().id()).collect::<Vec<_>>(),
                [3, 2]
            );
        }
    }
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

#[test]
fn spine_sizing_updates_column_geometry_without_waiting_for_client_commit() {
    let options: Rc<Options> = Rc::default();
    let mut space = scrolling(1., options.clone());
    let mut tile = scrolling_tile(1, &space, options);
    assert!(tile.set_spine(ShrinkAxis::Vertical, 40.));
    space.add_tile(None, tile, true, ColumnWidth::Fixed(400.), false, None);
    space.set_window_width(Some(&1), SizeChange::SetFixed(650));
    assert_eq!(space.tiles().next().unwrap().tile_size().w, 650.);
    assert_eq!(
        space.tiles().next().unwrap().window().requested_size(),
        None
    );
    // There is no client configure/commit that could fix stale ColumnData afterwards.
    space.verify_invariants();
    space.toggle_full_width();
    space.verify_invariants();
    space.toggle_window_width(Some(&1), true);
    space.verify_invariants();
    space.expand_column_to_available_width();
    space.verify_invariants();
}

#[test]
fn spine_sizing_height_uses_outer_geometry_and_ignores_client_minimum() {
    let options: Rc<Options> = Rc::default();
    let mut space = scrolling(1., options.clone());
    let mut tile = scrolling_tile(1, &space, options);
    assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
    space.add_tile(None, tile, true, ColumnWidth::Fixed(400.), false, None);
    let initial = space.tiles().next().unwrap().tile_size().h;
    space.set_window_height(Some(&1), SizeChange::AdjustFixed(-500));
    assert_eq!(space.tiles().next().unwrap().tile_size().h, initial - 500.);
    assert_eq!(space.tiles().next().unwrap().tile_size().w, 40.);
    assert_eq!(
        space.tiles().next().unwrap().window().requested_size(),
        None
    );
    space.verify_invariants();
}

#[test]
fn spine_sizing_floating_uses_outer_geometry_and_does_not_configure_client() {
    let mut tile = tile(1, 1., Rc::default());
    assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
    let mut space = FloatingSpace::new(
        (1280., 720.).into(),
        Rectangle::from_size((1280., 720.).into()),
        1.,
        tile.clock.clone(),
        Rc::default(),
    );
    space.add_tile(tile, true);
    let initial = space.tiles().next().unwrap().tile_size().h;
    space.set_window_height(Some(&1), SizeChange::AdjustFixed(-500), false);
    let tile = space.tiles().next().unwrap();
    assert_eq!(tile.tile_size().h, initial - 500.);
    assert_eq!(tile.tile_size().w, 40.);
    assert_eq!(tile.window().requested_size(), None);
    space.verify_invariants();
}

#[test]
fn ordinary_navigation_skips_spine_columns_and_hidden_tabs() {
    for display in [ColumnDisplay::Normal, ColumnDisplay::Tabbed] {
        let mut options = Options::default();
        options.layout.default_column_display = display;
        let options = Rc::new(options);
        let mut space = scrolling(1., options.clone());
        for id in 1..=5 {
            let mut tile = Tile::new(
                TestWindow::new(TestWindowParams::new(id)),
                (1280., 720.).into(),
                1.,
                space.clock().clone(),
                options.clone(),
            );
            if id == 2 || id == 4 {
                assert!(tile.set_spine(ShrinkAxis::Vertical, 40.));
            }
            if id <= 3 {
                if id == 1 {
                    space.add_tile(None, tile, true, ColumnWidth::Fixed(400.), false, None);
                } else {
                    space.add_tile_to_column(0, None, tile, false);
                }
            } else {
                space.add_tile(None, tile, false, ColumnWidth::Fixed(400.), false, None);
            }
        }
        settle(&mut space);
        assert!(space.activate_window(&1));
        assert!(space.focus_down());
        assert_eq!(space.active_window().unwrap().id(), &3);
        assert!(space.focus_up());
        assert_eq!(space.active_window().unwrap().id(), &1);
        space.focus_window_in_column(2);
        assert_eq!(space.active_window().unwrap().id(), &3);
        assert!(space.focus_right());
        assert_eq!(space.active_window().unwrap().id(), &5);
        assert!(space.focus_left());
        assert_eq!(space.active_window().unwrap().id(), &3);
        // Native internal selection may still select a spine, without expanding it.
        assert!(space.activate_window(&2));
        space.refresh(true, true);
        assert!(!space
            .tiles()
            .find(|t| t.window().id() == &2)
            .unwrap()
            .window()
            .0
            .pending_activated
            .get());
        space.focus_column_first();
        assert_eq!(space.active_window().unwrap().id(), &1);
        space.focus_column(2);
        assert_eq!(space.active_window().unwrap().id(), &5);
        assert_eq!(space.tiles().count(), 5);
        assert_eq!(space.tiles().filter(|t| t.is_spine()).count(), 2);
        space.verify_invariants();
    }
}

#[test]
fn all_spine_workspace_keeps_inventory_without_client_focus_or_sink_conversion() {
    for floating in [false, true] {
        let mut workspace =
            Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::default());
        let window = TestWindow::new(TestWindowParams::new(1));
        let mut tile = workspace.make_tile(window.clone());
        assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
        workspace.add_tile(
            tile,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            ColumnWidth::Fixed(400.),
            false,
            floating,
            None,
        );
        workspace.refresh(true, true);
        assert_eq!(workspace.selected_window().unwrap().id(), &1);
        assert!(workspace.active_window().is_none());
        assert!(!window.0.pending_activated.get());
        assert!(workspace.has_windows_or_name());
        assert!(workspace.has_window(&1));
        assert_eq!(workspace.windows().count(), 1);
        assert!(!workspace.focus_left());
        assert!(!workspace.focus_right());
        assert!(!workspace.focus_up());
        assert!(!workspace.focus_down());
        workspace.focus_column_first();
        workspace.focus_column_last();
        workspace.focus_window_top();
        workspace.focus_window_bottom();
        assert!(!workspace.sink_window(&1));
        assert_eq!(workspace.sink_count(), 0);
        assert_eq!(
            workspace.tiles().next().unwrap().form(),
            WindowForm::Spine(ShrinkAxis::Horizontal)
        );
        assert_eq!(window.requested_size(), None);
        workspace.verify_invariants(None);
    }
}

#[test]
fn floating_navigation_skips_spines_and_keeps_normal_distance_order() {
    let options: Rc<Options> = Rc::default();
    let clock = Clock::with_time(Duration::ZERO);
    let mut space = FloatingSpace::new(
        (1280., 720.).into(),
        Rectangle::from_size((1280., 720.).into()),
        1.,
        clock.clone(),
        options.clone(),
    );
    for id in 1..=3 {
        let mut tile = Tile::new(
            TestWindow::new(TestWindowParams::new(id)),
            (1280., 720.).into(),
            1.,
            clock.clone(),
            options.clone(),
        );
        if id == 2 {
            assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
        }
        space.add_tile(tile, id == 1);
        space.move_window(
            Some(&id),
            PositionChange::SetFixed(id as f64 * 200.),
            PositionChange::SetFixed(id as f64 * 100.),
            false,
        );
    }
    assert!(space.focus_right());
    assert_eq!(space.active_window().unwrap().id(), &3);
    assert!(space.focus_left());
    assert_eq!(space.active_window().unwrap().id(), &1);
    assert!(space.focus_down());
    assert_eq!(space.active_window().unwrap().id(), &3);
    space.focus_leftmost();
    assert_eq!(space.active_window().unwrap().id(), &1);
    space.focus_rightmost();
    assert_eq!(space.active_window().unwrap().id(), &3);
    space.activate_window(&2);
    space.refresh(true, true);
    assert!(!space
        .tiles()
        .find(|t| t.window().id() == &2)
        .unwrap()
        .window()
        .0
        .pending_activated
        .get());
    assert_eq!(space.tiles().count(), 3);
    space.verify_invariants();
}

#[test]
fn moving_spine_retains_native_owner_without_client_focus() {
    let mut layout = Layout::<TestWindow>::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    Op::Communicate(1).apply(&mut layout);
    let output = layout.outputs().next().unwrap().clone();
    let window = {
        let workspace = layout
            .workspaces_mut()
            .find(|ws| ws.has_window(&1))
            .unwrap();
        let tile = workspace
            .tiles_mut()
            .find(|t| t.window().id() == &1)
            .unwrap();
        assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
        tile.window().clone()
    };
    layout.update_window(&1, None);
    layout.refresh(true);
    assert!(layout.is_spine(&1));
    assert!(layout.focus().is_none());
    assert!(!layout.should_trigger_focus_follows_mouse_on(&1));
    assert!(layout.interactive_move_begin(1, &output, (25., 100.).into()));
    assert!(layout.interactive_move_update(&1, (300., 300.).into(), output, (325., 400.).into()));
    assert!(matches!(
        layout.interactive_move,
        Some(InteractiveMoveState::Moving(_))
    ));
    layout.refresh(true);
    assert!(layout.is_spine(&1));
    assert!(layout.focus_with_output().is_none());
    assert!(!layout.should_trigger_focus_follows_mouse_on(&1));
    assert!(!window.0.pending_activated.get());
    assert_eq!(layout.windows().count(), 1);
    layout.verify_invariants();
    layout.interactive_move_end(&1);
    layout.refresh(true);
    assert!(layout.is_spine(&1));
    assert!(layout.focus().is_none());
    let retained = layout.windows().find(|(_, w)| w.id() == &1).unwrap().1;
    assert!(Rc::ptr_eq(&retained.0, &window.0));
    layout.verify_invariants();
}

#[test]
fn native_move_anchor_tracks_spine_outer_geometry_and_preserves_normal_geometry() {
    for axis in [
        None,
        Some(ShrinkAxis::Horizontal),
        Some(ShrinkAxis::Vertical),
    ] {
        for floating in [false, true] {
            for scale in [1., 1.25] {
                for overview in [false, true] {
                    let mut options = Options::default();
                    options.layout.border.off = false;
                    options.layout.border.width = 7.3;
                    let mut layout = Layout::<TestWindow>::with_options(
                        Clock::with_time(Duration::ZERO),
                        options,
                    );
                    layout.clock.set_complete_instantly(true);
                    Op::AddScaledOutput {
                        id: 1,
                        scale,
                        layout_config: None,
                    }
                    .apply(&mut layout);
                    let mut params = TestWindowParams::new(1);
                    params.is_floating = floating;
                    Op::AddWindow { params }.apply(&mut layout);
                    Op::Communicate(1).apply(&mut layout);
                    let window = {
                        let ws = layout
                            .workspaces_mut()
                            .find(|ws| ws.has_window(&1))
                            .unwrap();
                        let tile = ws
                            .tiles_mut()
                            .find(|tile| tile.window().id() == &1)
                            .unwrap();
                        if let Some(axis) = axis {
                            assert!(tile.set_spine(axis, 40.3));
                        }
                        tile.window().clone()
                    };
                    layout.update_window(&1, None);
                    if overview {
                        layout.open_overview();
                    }
                    layout.advance_animations();
                    let output = layout.outputs().next().unwrap().clone();
                    let zoom = layout.overview_zoom();
                    let ratio = (0.23, 0.67);
                    let start = layout
                        .monitors()
                        .find_map(|mon| {
                            mon.workspaces_with_render_geo().find_map(|(ws, geo)| {
                                let (tile, pos, _) = ws
                                    .tiles_with_render_positions()
                                    .find(|(tile, _, _)| tile.window().id() == &1)?;
                                let (size, offset) = if axis.is_some() {
                                    (tile.tile_size(), Point::from((0., 0.)))
                                } else {
                                    (tile.window_size(), tile.window_loc())
                                };
                                Some(
                                    geo.loc
                                        + (pos
                                            + offset
                                            + Point::from((size.w * ratio.0, size.h * ratio.1)))
                                        .upscale(zoom),
                                )
                            })
                        })
                        .unwrap();
                    let requested = window.requested_size();
                    assert!(layout.interactive_move_begin(1, &output, start));
                    let Some(InteractiveMoveState::Starting {
                        pointer_ratio_within_window,
                        ..
                    }) = &layout.interactive_move
                    else {
                        panic!("expected native Starting owner")
                    };
                    approx::assert_abs_diff_eq!(
                        pointer_ratio_within_window.0,
                        ratio.0,
                        epsilon = 1e-9
                    );
                    approx::assert_abs_diff_eq!(
                        pointer_ratio_within_window.1,
                        ratio.1,
                        epsilon = 1e-9
                    );
                    let mut pointer = start + Point::from((255. * zoom, 0.));
                    assert!(layout.interactive_move_update(
                        &1,
                        (255. * zoom, 0.).into(),
                        output.clone(),
                        pointer
                    ));
                    if !floating {
                        assert!(matches!(
                            layout.interactive_move,
                            Some(InteractiveMoveState::Starting { .. })
                        ));
                        pointer.x += zoom;
                        assert!(layout.interactive_move_update(
                            &1,
                            (zoom, 0.).into(),
                            output,
                            pointer
                        ));
                    }
                    layout.advance_animations();
                    let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move else {
                        panic!("expected native Moving owner")
                    };
                    let (size, offset) = if axis.is_some() {
                        (move_.tile.tile_size(), Point::from((0., 0.)))
                    } else {
                        (move_.tile.window_size(), move_.tile.window_loc())
                    };
                    let anchor = move_.tile_render_location(zoom)
                        + (Point::from((size.w * ratio.0, size.h * ratio.1)) + offset
                            - move_.tile.render_offset())
                        .upscale(zoom);
                    approx::assert_abs_diff_eq!(anchor.x, pointer.x, epsilon = 0.5 / scale + 1e-9);
                    approx::assert_abs_diff_eq!(anchor.y, pointer.y, epsilon = 0.5 / scale + 1e-9);
                    if axis.is_some() {
                        let pose = move_.tile_render_location(zoom);
                        let size = move_.tile.tile_size();
                        window.0.forced_size.set(Some((1500, 1100).into()));
                        assert!(window.communicate());
                        layout.update_window(&1, None);
                        let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move
                        else {
                            unreachable!()
                        };
                        assert_eq!(move_.tile.tile_size(), size);
                        assert_eq!(move_.tile_render_location(zoom), pose);
                        assert_eq!(window.requested_size(), requested);
                    }
                    layout.verify_invariants();
                    layout.interactive_move_end(&1);
                    assert_eq!(layout.is_spine(&1), axis.is_some());
                    assert_eq!(layout.windows().count(), 1);
                    layout.verify_invariants();
                }
            }
        }
    }
}

#[test]
fn native_moving_floating_toggle_does_not_resize_spine_client() {
    for axis in [
        None,
        Some(ShrinkAxis::Horizontal),
        Some(ShrinkAxis::Vertical),
    ] {
        let mut layout = Layout::<TestWindow>::default();
        Op::AddOutput(1).apply(&mut layout);
        Op::AddWindow {
            params: TestWindowParams::new(1),
        }
        .apply(&mut layout);
        Op::Communicate(1).apply(&mut layout);
        let window = {
            let ws = layout
                .workspaces_mut()
                .find(|ws| ws.has_window(&1))
                .unwrap();
            let tile = ws
                .tiles_mut()
                .find(|tile| tile.window().id() == &1)
                .unwrap();
            if let Some(axis) = axis {
                assert!(tile.set_spine(axis, 40.));
            }
            tile.window().clone()
        };
        layout.update_window(&1, None);
        let output = layout.outputs().next().unwrap().clone();
        assert!(layout.interactive_move_begin(1, &output, (30., 100.).into()));
        assert!(layout.interactive_move_update(&1, (300., 0.).into(), output, (330., 100.).into()));
        let Some(InteractiveMoveState::Moving(move_)) = &mut layout.interactive_move else {
            unreachable!()
        };
        move_.tile.floating_window_size = Some((1111, 777).into());
        let before = window.requested_size();
        let outer = move_.tile.tile_size();
        layout.toggle_window_floating(Some(&1));
        let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move else {
            unreachable!()
        };
        assert!(move_.is_floating);
        if axis.is_some() {
            assert_eq!(window.requested_size(), before);
            assert_eq!(move_.tile.tile_size(), outer);
        } else {
            assert_eq!(window.requested_size(), Some((1111, 777).into()));
        }
        layout.interactive_move_end(&1);
        assert!(layout.active_workspace().unwrap().is_floating(&1));
        assert_eq!(layout.is_spine(&1), axis.is_some());
        let retained = layout.windows().find(|(_, w)| w.id() == &1).unwrap().1;
        assert!(Rc::ptr_eq(&retained.0, &window.0));
        layout.verify_invariants();
    }
}

#[test]
fn moving_book_mode_requests_do_not_partially_restore_or_change_client_intent() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let mut layout = Layout::<TestWindow>::default();
        Op::AddOutput(1).apply(&mut layout);
        Op::AddWindow {
            params: TestWindowParams::new(1),
        }
        .apply(&mut layout);
        Op::Communicate(1).apply(&mut layout);
        let ws = layout
            .workspaces_mut()
            .find(|ws| ws.has_window(&1))
            .unwrap();
        let tile = ws
            .tiles_mut()
            .find(|tile| tile.window().id() == &1)
            .unwrap();
        assert!(tile.set_spine(axis, 40.));
        let window = tile.window().clone();
        layout.update_window(&1, None);
        let output = layout.outputs().next().unwrap().clone();
        assert!(layout.interactive_move_begin(1, &output, (30., 100.).into()));
        assert!(layout.interactive_move_update(&1, (300., 0.).into(), output, (330., 100.).into()));
        let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move else {
            unreachable!()
        };
        let size = move_.tile.tile_size();
        let requested = window.requested_size();
        layout.request_fullscreen(&1, true);
        layout.request_maximized(&1, true);
        layout.request_fullscreen(&1, false);
        layout.request_maximized(&1, false);
        layout.request_toggle_fullscreen(&1);
        layout.request_toggle_maximized(&1);
        layout.request_toggle_windowed_fullscreen(&1);
        let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move else {
            unreachable!()
        };
        assert_eq!(move_.tile.form(), WindowForm::Spine(axis));
        assert_eq!(move_.tile.tile_size(), size);
        assert!(!move_.tile.has_normal_restore_allocation());
        assert_eq!(window.requested_size(), requested);
        assert_eq!(window.pending_sizing_mode(), SizingMode::Normal);
        assert!(!window.is_pending_windowed_fullscreen());
        assert_eq!(layout.windows().count(), 1);
        layout.verify_invariants();
        // Deferred mode intent after drop remains an E4 task; rejection is not the final feature.
        layout.interactive_move_end(&1);
        assert!(layout.is_spine(&1));
    }
}

#[test]
fn spine_is_not_a_cursor_warp_target_in_either_native_space() {
    for floating in [false, true] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            let mut layout = Layout::<TestWindow>::default();
            Op::AddOutput(1).apply(&mut layout);
            let mut params = TestWindowParams::new(1);
            params.is_floating = floating;
            Op::AddWindow { params }.apply(&mut layout);
            Op::Communicate(1).apply(&mut layout);
            let ws = layout
                .workspaces_mut()
                .find(|ws| ws.has_window(&1))
                .unwrap();
            assert!(ws.active_window_visual_rectangle().is_some());
            assert!(ws
                .tiles_mut()
                .find(|tile| tile.window().id() == &1)
                .unwrap()
                .set_spine(axis, 40.));
            ws.update_window(&1, None);
            assert!(ws.active_window().is_none());
            assert!(ws.active_window_visual_rectangle().is_none());
            // This internal form change is a control, not a user Expand action.
            assert!(ws
                .tiles_mut()
                .find(|tile| tile.window().id() == &1)
                .unwrap()
                .clear_spine());
            ws.update_window(&1, None);
            assert!(ws.active_window_visual_rectangle().is_some());
        }
    }
}

#[test]
fn floating_spine_resize_keeps_native_top_left_anchor_without_client_commit() {
    let mut tile = tile(1, 1., Rc::default());
    assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
    let mut space = FloatingSpace::new(
        (1280., 720.).into(),
        Rectangle::from_size((1280., 720.).into()),
        1.,
        tile.clock.clone(),
        Rc::default(),
    );
    space.add_tile(tile, true);
    let (_, before) = space.tiles_with_offsets().next().unwrap();
    let before_size = space.tiles().next().unwrap().tile_size();
    assert!(space.interactive_resize_begin(1, ResizeEdge::TOP_LEFT));
    assert!(space.interactive_resize_update(&1, (20., 100.).into()));
    let (tile, after) = space.tiles_with_offsets().next().unwrap();
    assert_eq!(tile.tile_size(), Size::from((40., before_size.h - 100.)));
    approx::assert_abs_diff_eq!(after.x, before.x, epsilon = 1e-9);
    approx::assert_abs_diff_eq!(after.y, before.y + 100., epsilon = 1e-9);
    assert_eq!(tile.window().requested_size(), None);
    space.verify_invariants();
    space.interactive_resize_end(Some(&1));
    space.verify_invariants();
}

#[test]
fn spine_width_preset_search_uses_outer_width_instead_of_hidden_client_width() {
    let mut options = Options::default();
    options.layout.preset_column_widths = vec![
        PresetSize::Fixed(300),
        PresetSize::Fixed(500),
        PresetSize::Fixed(700),
    ];
    let options = Rc::new(options);
    let mut space = scrolling(1., options.clone());
    let mut tile = scrolling_tile(1, &space, options);
    assert!(tile.set_spine(ShrinkAxis::Vertical, 40.));
    space.add_tile(None, tile, true, ColumnWidth::Fixed(400.), false, None);
    space.toggle_window_width(Some(&1), true);
    assert_eq!(space.tiles().next().unwrap().tile_size().w, 500.);
    assert_eq!(
        space.tiles().next().unwrap().window().requested_size(),
        None
    );
    space.verify_invariants();
}

#[test]
fn scrolling_spine_resize_uses_outer_extent_and_preserves_native_top_edge_rule() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let options: Rc<Options> = Rc::default();
        let mut space = scrolling(1., options.clone());
        let mut tile = scrolling_tile(1, &space, options);
        assert!(tile.set_spine(axis, 40.));
        space.add_tile(None, tile, true, ColumnWidth::Fixed(400.), false, None);
        let before = space.tiles().next().unwrap().tile_size();
        assert!(space.interactive_resize_begin(1, ResizeEdge::TOP));
        assert!(space.interactive_resize_update(&1, (0., 100.).into()));
        assert_eq!(space.tiles().next().unwrap().tile_size(), before);
        space.interactive_resize_end(Some(&1));
        assert!(space.interactive_resize_begin(1, ResizeEdge::BOTTOM_RIGHT));
        assert!(space.interactive_resize_update(&1, (50., -100.).into()));
        let tile = space.tiles().next().unwrap();
        let expected = match axis {
            ShrinkAxis::Horizontal => (40., before.h - 100.),
            ShrinkAxis::Vertical => (before.w + 50., 40.),
        };
        assert_eq!(tile.tile_size(), Size::from(expected));
        assert_eq!(tile.window().requested_size(), None);
        space.verify_invariants();
        space.interactive_resize_end(Some(&1));
    }
}

#[test]
fn ordinary_space_switch_finds_normal_receiver_and_skips_all_spine_space() {
    for target_floating in [false, true] {
        for normal_receiver in [false, true] {
            let mut workspace =
                Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::default());
            // A target may contain real windows but no client focus receiver.
            for (id, spine, floating) in [
                (1, false, !target_floating),
                (2, !normal_receiver, target_floating),
                (3, true, target_floating),
            ] {
                let mut tile = workspace.make_tile(TestWindow::new(TestWindowParams::new(id)));
                if spine {
                    assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
                }
                workspace.add_tile(
                    tile,
                    WorkspaceAddWindowTarget::Auto,
                    ActivateWindow::Yes,
                    ColumnWidth::Fixed(400.),
                    false,
                    floating,
                    None,
                );
            }
            workspace.activate_window(&1);
            workspace.switch_focus_floating_tiling();
            let expected = if normal_receiver { 2 } else { 1 };
            assert_eq!(workspace.active_window().unwrap().id(), &expected);
            assert_eq!(workspace.windows().count(), 3);
            assert_eq!(
                workspace.tiles().filter(|t| t.is_spine()).count(),
                if normal_receiver { 1 } else { 2 }
            );
            // An explicit request has the same ordinary focus semantics.
            workspace.activate_window(&1);
            if target_floating {
                workspace.focus_floating();
            } else {
                workspace.focus_tiling();
            }
            assert_eq!(workspace.active_window().unwrap().id(), &expected);
            workspace.refresh(true, true);
            for tile in workspace.tiles().filter(|tile| tile.is_spine()) {
                assert!(!tile.window().0.pending_activated.get());
            }
            workspace.verify_invariants(None);
        }
    }
}

#[test]
fn axis_plans_are_read_only_and_match_committed_native_column_sizes() {
    for scale in [1., 1.25] {
        for display in [ColumnDisplay::Normal, ColumnDisplay::Tabbed] {
            for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
                let mut options = Options::default();
                options.layout.default_column_display = display;
                let options = Rc::new(options);
                let mut space = scrolling(scale, options.clone());
                let tile = scrolling_tile(1, &space, options.clone());
                let client = tile.window().clone();
                space.add_tile(None, tile, true, ColumnWidth::Fixed(800.), false, None);
                let other = scrolling_tile(2, &space, options);
                let other_client = other.window().clone();
                space.add_tile_to_column(0, None, other, false);
                settle(&mut space);
                let before = [client.requested_size(), other_client.requested_size()];
                let sizes = space.tiles().map(|t| t.tile_size()).collect::<Vec<_>>();
                let plans = space.spine_plans(&1, 40.1).unwrap();
                assert_eq!(
                    [client.requested_size(), other_client.requested_size()],
                    before
                );
                assert_eq!(
                    space.tiles().map(|t| t.tile_size()).collect::<Vec<_>>(),
                    sizes
                );
                assert!(space.tiles().all(|t| !t.is_spine()));
                assert_eq!(
                    plans.first().unwrap().geometry.axis,
                    if display == ColumnDisplay::Normal {
                        ShrinkAxis::Vertical
                    } else {
                        ShrinkAxis::Horizontal
                    }
                );
                assert!(plans.get(axis).fits);
                assert_eq!(plans.next(axis).unwrap().geometry.axis, axis.other());
                assert_eq!(plans.for_move(axis).geometry.axis, axis);
                space
                    .tiles_mut()
                    .find(|t| t.window().id() == &1)
                    .unwrap()
                    .set_spine(axis, 40.1);
                space.update_window(&1, None);
                assert_eq!(
                    space
                        .tiles()
                        .find(|t| t.window().id() == &1)
                        .unwrap()
                        .tile_size(),
                    plans.get(axis).geometry.outer_size()
                );
                settle(&mut space);
            }
        }
    }
}

#[test]
fn insertion_plans_match_native_new_column_and_in_column_without_reselecting_topology() {
    for scale in [1., 1.25] {
        for display in [ColumnDisplay::Normal, ColumnDisplay::Tabbed] {
            for in_column in [false, true] {
                for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
                    let mut options = Options::default();
                    options.layout.default_column_display = display;
                    let options = Rc::new(options);
                    let mut space = scrolling(scale, options.clone());
                    let first = scrolling_tile(1, &space, options.clone());
                    space.add_tile(None, first, true, ColumnWidth::Fixed(800.), false, None);
                    settle(&mut space);
                    let mut incoming = scrolling_tile(2, &space, options);
                    let client = incoming.window().clone();
                    let plans = if in_column {
                        space.spine_in_column_plans(0, 1, &incoming, 40.1)
                    } else {
                        space.spine_new_column_plans(
                            &incoming,
                            ColumnWidth::Fixed(700.),
                            false,
                            40.1,
                        )
                    }
                    .unwrap();
                    assert_eq!(client.requested_size(), None);
                    assert_eq!(space.tiles().count(), 1);
                    assert_eq!(space.columns().count(), 1);
                    assert!(incoming.set_spine(axis, 40.1));
                    if in_column {
                        space.add_tile_to_column(0, Some(1), incoming, true);
                    } else {
                        space.add_tile(
                            Some(1),
                            incoming,
                            true,
                            ColumnWidth::Fixed(700.),
                            false,
                            None,
                        );
                    }
                    assert_eq!(space.columns().count(), if in_column { 1 } else { 2 });
                    assert_eq!(
                        space
                            .tiles()
                            .find(|t| t.window().id() == &2)
                            .unwrap()
                            .tile_size(),
                        plans.get(axis).geometry.outer_size()
                    );
                    assert_eq!(client.requested_size(), None);
                    settle(&mut space);
                }
            }
        }
    }
}

#[test]
fn axis_legality_precedes_local_preference_when_native_neighbor_is_oversized() {
    let options: Rc<Options> = Rc::default();
    let mut space = ScrollingSpace::new(
        (320., 720.).into(),
        Rectangle::from_size((320., 720.).into()),
        1.,
        Clock::with_time(Duration::ZERO),
        options.clone(),
    );
    let tile = scrolling_tile(1, &space, options.clone());
    space.add_tile(None, tile, true, ColumnWidth::Fixed(800.), false, None);
    let mut incoming = scrolling_tile(2, &space, options);
    let plans = space.spine_in_column_plans(0, 1, &incoming, 40.).unwrap();
    assert!(plans.get(ShrinkAxis::Horizontal).fits);
    assert!(!plans.get(ShrinkAxis::Vertical).fits);
    assert_eq!(plans.first().unwrap().geometry.axis, ShrinkAxis::Horizontal);
    assert_eq!(
        plans.next(ShrinkAxis::Horizontal).unwrap().geometry.axis,
        ShrinkAxis::Horizontal
    );
    let chosen = plans.for_move(ShrinkAxis::Vertical);
    assert_eq!(chosen.geometry.axis, ShrinkAxis::Horizontal);
    assert!(incoming.set_spine(chosen.geometry.axis, 40.));
    space.add_tile_to_column(0, Some(1), incoming, true);
    assert_eq!(space.columns().count(), 1);
    assert_eq!(
        space
            .tiles()
            .find(|t| t.window().id() == &2)
            .unwrap()
            .tile_size(),
        chosen.geometry.outer_size()
    );
    settle(&mut space);
}

#[test]
fn both_axes_too_small_keep_move_axis_and_use_native_overflow_allocation() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let options: Rc<Options> = Rc::default();
        let mut space = ScrollingSpace::new(
            (20., 20.).into(),
            Rectangle::from_size((20., 20.).into()),
            1.,
            Clock::with_time(Duration::ZERO),
            options.clone(),
        );
        let mut incoming = scrolling_tile(1, &space, options);
        let plans = space
            .spine_new_column_plans(&incoming, ColumnWidth::Fixed(400.), false, 40.)
            .unwrap();
        assert!(plans.first().is_none());
        assert!(plans.next(axis).is_none());
        let chosen = plans.for_move(axis);
        assert_eq!(chosen.geometry.axis, axis);
        assert!(!chosen.fits);
        assert!(incoming.set_spine(axis, 40.));
        space.add_tile(None, incoming, true, ColumnWidth::Fixed(400.), false, None);
        assert_eq!(
            space.tiles().next().unwrap().tile_size(),
            chosen.geometry.outer_size()
        );
        assert_eq!(space.columns().count(), 1);
        settle(&mut space);
    }
}

#[test]
fn sibling_minimum_heights_can_make_both_axes_illegal_without_changing_native_insertion() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let options: Rc<Options> = Rc::default();
        let mut space = scrolling(1., options.clone());
        let first = scrolling_tile(1, &space, options.clone());
        space.add_tile(None, first, true, ColumnWidth::Fixed(800.), false, None);
        let second = scrolling_tile(2, &space, options.clone());
        space.add_tile_to_column(0, None, second, false);
        settle(&mut space);
        let mut incoming = scrolling_tile(3, &space, options);
        let plans = space.spine_in_column_plans(0, 2, &incoming, 40.).unwrap();
        assert!(!plans.get(ShrinkAxis::Horizontal).fits);
        assert!(!plans.get(ShrinkAxis::Vertical).fits);
        assert!(plans.first().is_none());
        let chosen = plans.for_move(axis);
        assert_eq!(chosen.geometry.axis, axis);
        assert!(incoming.set_spine(axis, 40.));
        space.add_tile_to_column(0, Some(2), incoming, true);
        assert_eq!(space.columns().count(), 1);
        assert_eq!(space.tiles().count(), 3);
        assert_eq!(
            space
                .tiles()
                .find(|t| t.window().id() == &3)
                .unwrap()
                .tile_size(),
            chosen.geometry.outer_size()
        );
        settle(&mut space);
    }
}

#[test]
fn floating_axis_plans_are_read_only_and_default_to_horizontal_shrink() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let options: Rc<Options> = Rc::default();
        let tile = tile(1, 1., options.clone());
        let client = tile.window().clone();
        let mut space = FloatingSpace::new(
            (1280., 720.).into(),
            Rectangle::from_size((1280., 720.).into()),
            1.,
            tile.clock.clone(),
            options,
        );
        space.add_tile(tile, true);
        let before = client.requested_size();
        let plans = space.spine_plans(&1, 40.).unwrap();
        assert_eq!(plans.first().unwrap().geometry.axis, ShrinkAxis::Horizontal);
        assert_eq!(client.requested_size(), before);
        assert!(!space.tiles().next().unwrap().is_spine());
        let tile = space.tiles_mut().next().unwrap();
        assert!(tile.set_spine(axis, 40.));
        space.update_window(&1, None);
        assert_eq!(
            space.tiles().next().unwrap().tile_size(),
            plans.get(axis).geometry.outer_size()
        );
        assert_eq!(client.requested_size(), before);
        space.verify_invariants();
    }
}

#[test]
fn initial_shrink_query_rejects_transients_sink_spines_and_non_normal_modes_without_mutation() {
    let mut layout = Layout::<TestWindow>::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    assert!(layout.shrink_plans(&1, 40.).unwrap().first().is_some());
    assert!(layout.shrink_plans(&99, 40.).is_none());
    assert!(layout.shrink_plans(&1, f64::NAN).is_none());
    let window = layout
        .windows()
        .find(|(_, w)| w.id() == &1)
        .unwrap()
        .1
        .clone();
    let before = window.requested_size();
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        window.0.pending_sizing_mode.set(mode);
        assert!(layout.shrink_plans(&1, 40.).is_none());
        assert_eq!(window.requested_size(), before);
    }
    window.0.pending_sizing_mode.set(SizingMode::Normal);
    let mut params = TestWindowParams::new(2);
    params.parent_id = Some(1);
    Op::AddWindow { params }.apply(&mut layout);
    assert!(layout.shrink_plans(&1, 40.).is_none());
    assert!(layout.shrink_plans(&2, 40.).is_none());
    let child = layout
        .windows()
        .find(|(_, w)| w.id() == &2)
        .unwrap()
        .1
        .clone();
    child.0.parent_id.set(Some(99));
    assert!(layout.shrink_plans(&2, 40.).is_none());
    child.0.parent_id.set(None);
    assert!(layout.shrink_plans(&1, 40.).is_some());
    let workspace = layout
        .workspaces_mut()
        .find(|ws| ws.has_window(&1))
        .unwrap();
    assert!(workspace.sink_window(&1));
    assert!(layout.shrink_plans(&1, 40.).is_none());
    let workspace = layout
        .workspaces_mut()
        .find(|ws| ws.has_window(&2))
        .unwrap();
    let tile = workspace
        .tiles_mut()
        .find(|t| t.window().id() == &2)
        .unwrap();
    assert!(tile.set_spine(ShrinkAxis::Horizontal, 40.));
    assert!(layout.shrink_plans(&2, 40.).is_none());
    assert_eq!(layout.windows().count(), 2);
    layout.update_window(&2, None);
    layout.verify_invariants();
}

#[test]
fn normal_restore_submits_live_default_at_same_slot_and_preserves_absent_focus() {
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let mut options = Options::default();
        options.layout.default_column_width = Some(PresetSize::Fixed(760));
        let mut workspace =
            Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::new(options));
        let client = TestWindow::new(TestWindowParams::new(1));
        let mut book = workspace.make_tile(client.clone());
        book.floating_window_size = Some((999, 888).into());
        assert!(book.set_spine(axis, 40.));
        workspace.add_tile(
            book,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            ColumnWidth::Fixed(1100.),
            true,
            false,
            None,
        );
        assert!(workspace.active_window().is_none());
        let expected = workspace.normal_spine_plans(&[1])[0].1;
        assert_eq!(workspace.restore_normal_spines(&[99, 1, 1]), vec![1]);
        assert_eq!(client.requested_size(), Some(expected.client_size));
        assert_eq!(workspace.tiles().next().unwrap().form(), WindowForm::Normal);
        assert!(workspace.active_window().is_none());
        assert_eq!(workspace.windows().count(), 1);
        assert_eq!(workspace.scrolling().columns().count(), 1);
        assert_eq!(
            workspace.tiles().next().unwrap().floating_window_size,
            Some((999, 888).into())
        );
        assert!(workspace.restore_normal_spines(&[1]).is_empty());
        for _ in 0..4 {
            if client.communicate() {
                workspace.update_window(&1, None);
            }
        }
        workspace.verify_invariants(None);
        workspace.refresh(true, true);
        assert!(!client.0.pending_activated.get());
        assert!(workspace.active_window().is_none());
        workspace.activate_window(&1);
        workspace.refresh(true, true);
        assert_eq!(workspace.active_window().unwrap().id(), &1);
        assert!(client.0.pending_activated.get());
    }
}

#[test]
fn normal_restore_batch_reuses_shared_column_solver_and_keeps_existing_focus() {
    for display in [ColumnDisplay::Normal, ColumnDisplay::Tabbed] {
        let mut options = Options::default();
        options.layout.default_column_display = display;
        let mut workspace =
            Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), Rc::new(options));
        let mut clients = Vec::new();
        for id in 1..=3 {
            let mut params = TestWindowParams::new(id);
            params.min_max_size = ((100, 20).into(), (0, 0).into());
            let client = TestWindow::new(params);
            let mut tile = workspace.make_tile(client.clone());
            if id > 1 {
                assert!(tile.set_spine(ShrinkAxis::Vertical, 40.));
            }
            if id == 1 {
                workspace.add_tile(
                    tile,
                    WorkspaceAddWindowTarget::Auto,
                    ActivateWindow::Yes,
                    ColumnWidth::Fixed(700.),
                    false,
                    false,
                    None,
                );
            } else {
                workspace
                    .scrolling_mut()
                    .add_tile_to_column(0, None, tile, false);
            }
            clients.push(client);
        }
        workspace
            .scrolling_mut()
            .set_window_height(Some(&2), SizeChange::SetFixed(200));
        let plans = workspace.normal_spine_plans(&[3, 2]);
        let focus = *workspace.active_window().unwrap().id();
        assert_eq!(workspace.restore_normal_spines(&[3, 2, 1, 2]), vec![2, 3]);
        for (id, plan) in &plans {
            assert_eq!(clients[id - 1].requested_size(), Some(plan.client_size));
        }
        for _ in 0..4 {
            for client in &clients {
                if client.communicate() {
                    workspace.update_window(client.id(), None);
                }
            }
        }
        workspace.verify_invariants(None);
        assert_eq!(*workspace.active_window().unwrap().id(), focus);
        assert_eq!(workspace.scrolling().columns().count(), 1);
        assert_eq!(
            workspace
                .tiles()
                .map(|t| *t.window().id())
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }
}

#[test]
fn normal_restore_in_non_normal_container_keeps_origin_until_mode_commit() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let mut space = non_normal_space(1.25, mode, ColumnDisplay::Tabbed);
        let options = space.tiles().next().unwrap().options().clone();
        let mut book = scrolling_tile(2, &space, options);
        assert!(book.set_spine(ShrinkAxis::Vertical, 40.));
        space.add_tile_to_column(0, None, book, true);
        space.remove_tile(&1, Transaction::new());
        let requests = vec![(2, ColumnWidth::Fixed(760.))];
        let plans = space.normal_spine_plans(&requests);
        assert!(space
            .tiles_mut()
            .next()
            .unwrap()
            .prepare_normal_restore(plans[0].1));
        space.commit_normal_restore(&plans, &requests, Transaction::new(), true);
        space.verify_invariants();
        assert_eq!(space.tiles().next().unwrap().sizing_mode(), mode);
        assert_eq!(
            space.tiles().next().unwrap().window().pending_sizing_mode(),
            mode
        );
        settle(&mut space);
        assert_eq!(space.tiles().next().unwrap().window().sizing_mode(), mode);
        space.verify_invariants();
    }
}

#[test]
fn floating_restore_requests_normal_defaults_and_preserves_logical_position_and_order() {
    for explicit_size in [false, true] {
        let options = Rc::new(Options::default());
        let mut workspace = Workspace::new_no_outputs(Clock::with_time(Duration::ZERO), options);
        let mut params = TestWindowParams::new(1);
        params.bbox = Rectangle::from_size((240, 160).into());
        if explicit_size {
            params.rules = Some(ResolvedWindowRules {
                default_width: Some(Some(PresetSize::Fixed(720))),
                default_height: Some(Some(PresetSize::Fixed(400))),
                ..Default::default()
            });
        }
        let client = TestWindow::new(params);
        let mut book = workspace.make_tile(client.clone());
        book.floating_window_size = Some((999, 888).into());
        book.floating_pos = Some((0.2, 0.3).into());
        assert!(book.set_spine(ShrinkAxis::Horizontal, 40.));
        workspace.add_tile(
            book,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            ColumnWidth::Fixed(900.),
            false,
            true,
            None,
        );
        let before = workspace
            .floating()
            .tiles_with_offsets()
            .map(|(t, p)| (*t.window().id(), p))
            .collect::<Vec<_>>();
        let plan = workspace.normal_spine_plans(&[1])[0].1;
        assert_eq!(workspace.restore_normal_spines(&[1]), vec![1]);
        assert_eq!(client.requested_size(), Some(plan.client_size));
        assert_eq!(
            plan.client_size,
            if explicit_size {
                (720, 400).into()
            } else {
                (0, 0).into()
            }
        );
        assert_eq!(
            workspace
                .floating()
                .tiles_with_offsets()
                .map(|(t, p)| (*t.window().id(), p))
                .collect::<Vec<_>>(),
            before
        );
        for _ in 0..4 {
            if client.communicate() {
                workspace.update_window(&1, None);
            }
        }
        workspace.verify_invariants(None);
        assert_eq!(
            workspace
                .floating()
                .tiles_with_offsets()
                .map(|(t, p)| (*t.window().id(), p))
                .collect::<Vec<_>>(),
            before
        );
        assert!(workspace.active_window().is_none());
    }
}

#[test]
fn normal_restore_mode_unset_before_first_commit_keeps_native_view_restore_record() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let mut space = non_normal_space(1.25, mode, ColumnDisplay::Tabbed);
        let options = space.tiles().next().unwrap().options().clone();
        let mut book = scrolling_tile(2, &space, options);
        assert!(book.set_spine(ShrinkAxis::Vertical, 40.));
        space.add_tile_to_column(0, None, book, true);
        space.remove_tile(&1, Transaction::new());
        let requests = vec![(2, ColumnWidth::Fixed(760.))];
        let plans = space.normal_spine_plans(&requests);
        assert!(space
            .tiles_mut()
            .next()
            .unwrap()
            .prepare_normal_restore(plans[0].1));
        space.commit_normal_restore(&plans, &requests, Transaction::new(), true);
        // The client is still Normal; the column origin came from the former spine-only mode.
        assert_eq!(
            space.tiles().next().unwrap().window().sizing_mode(),
            SizingMode::Normal
        );
        if mode.is_fullscreen() {
            space.set_fullscreen(&2, false);
        } else {
            space.set_maximized(&2, false);
        }
        space.verify_invariants();
        settle(&mut space);
        space.verify_invariants();
        assert_eq!(
            space.tiles().next().unwrap().window().sizing_mode(),
            SizingMode::Normal
        );
    }
}

fn explicit_mode_layout(display: ColumnDisplay) -> Layout<TestWindow> {
    let mut options = Options::default();
    options.layout.default_column_display = display;
    let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    Op::Communicate(1).apply(&mut layout);
    let ws = layout
        .workspaces_mut()
        .find(|ws| ws.has_window(&1))
        .unwrap();
    for (id, axis) in [(2, ShrinkAxis::Horizontal), (3, ShrinkAxis::Vertical)] {
        let mut tile = ws.make_tile(TestWindow::new(TestWindowParams::new(id)));
        assert!(tile.set_spine(axis, 40.));
        ws.scrolling_mut().add_tile_to_column(0, None, tile, false);
    }
    layout.verify_invariants();
    layout
}

#[test]
fn explicit_duplicate_column_mode_restores_all_tabs_before_native_early_return() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let mut layout = explicit_mode_layout(ColumnDisplay::Tabbed);
        if mode.is_fullscreen() {
            layout.set_fullscreen(&1, true);
        } else {
            layout.set_maximized(&1, true);
        }
        Op::Communicate(1).apply(&mut layout);
        assert!(layout.is_spine(&2) && layout.is_spine(&3));
        if mode.is_fullscreen() {
            layout.request_fullscreen(&1, true);
        } else {
            layout.request_maximized(&1, true);
        }
        for id in [2, 3] {
            assert!(!layout.is_spine(&id));
            let win = layout.windows().find(|(_, w)| w.id() == &id).unwrap().1;
            assert_eq!(win.pending_sizing_mode(), mode);
            assert_eq!(
                win.requested_size(),
                layout
                    .windows()
                    .find(|(_, w)| w.id() == &1)
                    .unwrap()
                    .1
                    .requested_size()
            );
            Op::Communicate(id).apply(&mut layout);
        }
        assert_eq!(layout.windows().count(), 3);
        assert_eq!(layout.focus().unwrap().id(), &1);
        layout.verify_invariants();
    }
}

#[test]
fn explicit_toggle_uses_book_container_flags_before_restore_including_latent_maximize() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let mut layout = explicit_mode_layout(ColumnDisplay::Tabbed);
        if mode.is_fullscreen() {
            layout.set_fullscreen(&1, true);
        } else {
            layout.set_maximized(&1, true);
        }
        Op::Communicate(1).apply(&mut layout);
        assert_eq!(
            layout
                .windows()
                .find(|(_, w)| w.id() == &2)
                .unwrap()
                .1
                .pending_sizing_mode(),
            SizingMode::Normal
        );
        if mode.is_fullscreen() {
            layout.request_toggle_fullscreen(&2);
        } else {
            layout.request_toggle_maximized(&2);
        }
        let ws = layout
            .workspaces()
            .find(|(_, _, ws)| ws.has_window(&1))
            .unwrap()
            .2;
        let col = ws.scrolling().columns().next().unwrap();
        assert!(!col.is_pending_fullscreen() && !col.is_pending_maximized());
        assert!(!layout.is_spine(&2) && !layout.is_spine(&3));
        for id in [1, 2, 3] {
            Op::Communicate(id).apply(&mut layout);
        }
        layout.verify_invariants();
    }
    let mut layout = explicit_mode_layout(ColumnDisplay::Tabbed);
    layout.set_maximized(&1, true);
    layout.set_fullscreen(&1, true);
    Op::Communicate(1).apply(&mut layout);
    layout.request_toggle_maximized(&2);
    let ws = layout
        .workspaces()
        .find(|(_, _, ws)| ws.has_window(&1))
        .unwrap()
        .2;
    let col = ws.scrolling().columns().next().unwrap();
    assert!(col.is_pending_fullscreen());
    assert!(!col.is_pending_maximized());
    for id in [1, 2, 3] {
        Op::Communicate(id).apply(&mut layout);
    }
    layout.verify_invariants();
}

#[test]
fn explicit_mode_on_normal_column_keeps_native_extraction_and_other_spines() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let mut layout = explicit_mode_layout(ColumnDisplay::Normal);
        let other = layout
            .windows()
            .find(|(_, w)| w.id() == &3)
            .unwrap()
            .1
            .clone();
        if mode.is_fullscreen() {
            layout.request_fullscreen(&2, true);
        } else {
            layout.request_maximized(&2, true);
        }
        assert!(!layout.is_spine(&2));
        assert!(layout.is_spine(&3));
        assert_eq!(other.requested_size(), None);
        let ws = layout
            .workspaces()
            .find(|(_, _, ws)| ws.has_window(&1))
            .unwrap()
            .2;
        assert_eq!(ws.scrolling().columns().count(), 2);
        assert_eq!(
            layout
                .windows()
                .find(|(_, w)| w.id() == &2)
                .unwrap()
                .1
                .pending_sizing_mode(),
            mode
        );
        for id in [1, 2] {
            Op::Communicate(id).apply(&mut layout);
        }
        layout.verify_invariants();
        assert_eq!(layout.windows().count(), 3);
    }
}

#[test]
fn explicit_windowed_fullscreen_restores_only_affected_scope_and_waits_for_flag_commit() {
    for fullscreen_container in [false, true] {
        let mut layout = explicit_mode_layout(ColumnDisplay::Tabbed);
        if fullscreen_container {
            layout.set_fullscreen(&1, true);
            Op::Communicate(1).apply(&mut layout);
        }
        layout.request_toggle_windowed_fullscreen(&2);
        assert!(!layout.is_spine(&2));
        assert_eq!(layout.is_spine(&3), !fullscreen_container);
        let ws = layout
            .workspaces()
            .find(|(_, _, ws)| ws.has_window(&1))
            .unwrap()
            .2;
        let tile = ws.tiles().find(|t| t.window().id() == &2).unwrap();
        assert!(tile.window().is_pending_windowed_fullscreen());
        assert!(!tile.window().is_windowed_fullscreen());
        assert!(tile.normal_restore_input_blocked());
        for id in [1, 2, 3] {
            Op::Communicate(id).apply(&mut layout);
        }
        layout.verify_invariants();
        layout.request_toggle_windowed_fullscreen(&2);
        Op::Communicate(2).apply(&mut layout);
        assert!(!layout
            .windows()
            .find(|(_, w)| w.id() == &2)
            .unwrap()
            .1
            .is_windowed_fullscreen());
        layout.verify_invariants();
    }
}

#[test]
fn explicit_floating_modes_restore_form_and_keep_native_return_to_floating() {
    for mode in [SizingMode::Fullscreen, SizingMode::Maximized] {
        let mut layout = Layout::<TestWindow>::default();
        Op::AddOutput(1).apply(&mut layout);
        let mut params = TestWindowParams::new(1);
        params.is_floating = true;
        Op::AddWindow { params }.apply(&mut layout);
        Op::Communicate(1).apply(&mut layout);
        let ws = layout
            .workspaces_mut()
            .find(|ws| ws.has_window(&1))
            .unwrap();
        assert!(ws.is_floating(&1));
        assert!(ws
            .tiles_mut()
            .next()
            .unwrap()
            .set_spine(ShrinkAxis::Horizontal, 40.));
        ws.update_window(&1, None);
        let win = layout.windows().next().unwrap().1.clone();
        let output_events = win.0.output_events.borrow().clone();
        if mode.is_fullscreen() {
            layout.request_fullscreen(&1, true);
        } else {
            layout.request_maximized(&1, true);
        }
        assert!(!layout.is_spine(&1));
        assert!(!layout
            .workspaces()
            .find(|(_, _, ws)| ws.has_window(&1))
            .unwrap()
            .2
            .is_floating(&1));
        assert_eq!(win.pending_sizing_mode(), mode);
        Op::Communicate(1).apply(&mut layout);
        layout.verify_invariants();
        if mode.is_fullscreen() {
            layout.request_fullscreen(&1, false);
        } else {
            layout.request_maximized(&1, false);
        }
        Op::Communicate(1).apply(&mut layout);
        layout.verify_invariants();
        assert!(layout
            .workspaces()
            .find(|(_, _, ws)| ws.has_window(&1))
            .unwrap()
            .2
            .is_floating(&1));
        assert_eq!(
            win.0.output_events.borrow().as_slice(),
            output_events.as_slice()
        );
        assert!(Rc::ptr_eq(&win.0, &layout.windows().next().unwrap().1 .0));
    }
}

#[test]
fn explicit_unfullscreen_of_windowed_client_keeps_other_book_tabs_collapsed() {
    let mut layout = explicit_mode_layout(ColumnDisplay::Tabbed);
    layout.request_toggle_windowed_fullscreen(&2);
    Op::Communicate(2).apply(&mut layout);
    assert!(layout.is_spine(&3));
    layout.request_fullscreen(&2, false);
    assert!(layout.is_spine(&3));
    layout.request_fullscreen(&2, false);
    assert!(layout.is_spine(&3));
    Op::Communicate(2).apply(&mut layout);
    let win = layout.windows().find(|(_, w)| w.id() == &2).unwrap().1;
    assert!(!win.is_windowed_fullscreen());
    assert!(!win.is_pending_windowed_fullscreen());
    layout.request_fullscreen(&2, false);
    layout.request_maximized(&2, false);
    assert!(layout.is_spine(&3));
    layout.verify_invariants();
}
