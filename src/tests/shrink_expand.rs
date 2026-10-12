use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_toplevel;
use smithay::reexports::wayland_server::Resource as _;
use wayland_client::Proxy as _;

use super::Fixture;
use crate::layout::shrink_expand::{ShrinkAxis, WindowForm};
use crate::layout::LayoutElement as _;
use crate::layout::SizingMode;

struct ShrinkIpcClient(std::io::BufReader<std::os::unix::net::UnixStream>);

fn spatial_test_pointer(
    fixture: &mut Fixture,
    edges: crate::utils::ResizeEdge,
) -> (
    smithay::desktop::Window,
    smithay::input::pointer::PointerHandle<crate::niri::State>,
) {
    let window = fixture.niri().layout.focus().unwrap().window.clone();
    spatial_test_pointer_for_window(fixture, edges, window)
}

fn spatial_test_pointer_for_window(
    fixture: &mut Fixture,
    edges: crate::utils::ResizeEdge,
    window: smithay::desktop::Window,
) -> (
    smithay::desktop::Window,
    smithay::input::pointer::PointerHandle<crate::niri::State>,
) {
    use crate::input::{resize_grab::ResizeGrab, AnyStartData};
    use smithay::input::pointer::{ButtonEvent, Focus, GrabStartData};
    use smithay::utils::Serial;
    assert!(fixture
        .niri()
        .layout
        .interactive_resize_begin(window.clone(), edges));
    let pointer = fixture.niri().seat.get_pointer().unwrap();
    pointer.button(
        fixture.niri_state(),
        &ButtonEvent {
            serial: Serial::from(400),
            time: smithay::backend::input::InputTime::now(),
            button: 0x110,
            state: smithay::backend::input::ButtonState::Pressed,
        },
    );
    pointer.set_grab(
        fixture.niri_state(),
        ResizeGrab::new(
            AnyStartData::Pointer(GrabStartData {
                focus: None,
                button: 0x110,
                location: (400., 300.).into(),
            }),
            window.clone(),
        ),
        Serial::from(401),
        Focus::Clear,
    );
    (window, pointer)
}

fn spatial_test_motion(
    fixture: &mut Fixture,
    pointer: &smithay::input::pointer::PointerHandle<crate::niri::State>,
    delta: (f64, f64),
) {
    pointer.motion(
        fixture.niri_state(),
        None,
        &smithay::input::pointer::MotionEvent {
            location: (400. + delta.0, 300. + delta.1).into(),
            serial: smithay::utils::Serial::from(402),
            time: smithay::backend::input::InputTime::now(),
        },
    );
    pointer.frame(fixture.niri_state());
}

fn spine_test_point(
    fixture: &mut Fixture,
    window: &smithay::desktop::Window,
    ratio: (f64, f64),
) -> smithay::utils::Point<f64, smithay::utils::Logical> {
    if fixture
        .niri()
        .layout
        .workspaces()
        .any(|(_, _, ws)| ws.floating().has_window(window))
    {
        // The two fixture floats start at the same position. Expose the book rather than
        // expecting a click to pass through the ordinary window raised by shrink handoff.
        fixture.niri().layout.move_floating_window(
            Some(window),
            niri_ipc::PositionChange::SetFixed(900.),
            niri_ipc::PositionChange::SetFixed(350.),
            false,
        );
    }
    let niri = fixture.niri();
    let output = niri
        .layout
        .workspaces()
        .find(|(_, _, ws)| ws.has_window(window))
        .unwrap()
        .0
        .unwrap()
        .output()
        .clone();
    let mon = niri.layout.monitor_for_output(&output).unwrap();
    let (ws, geo) = mon
        .workspaces_with_render_geo()
        .find(|(ws, _)| ws.has_window(window))
        .unwrap();
    let (tile, pos, _) = ws
        .tiles_with_render_positions()
        .find(|(t, _, _)| &t.window().window == window)
        .unwrap();
    let size = tile.tile_size();
    let offset = smithay::utils::Point::from((size.w * ratio.0, size.h * ratio.1));
    niri.global_space
        .output_geometry(&output)
        .unwrap()
        .loc
        .to_f64()
        + geo.loc
        + (pos + offset).upscale(mon.overview_zoom())
}

fn spine_test_press(
    fixture: &mut Fixture,
    pos: smithay::utils::Point<f64, smithay::utils::Logical>,
) -> smithay::input::pointer::PointerHandle<crate::niri::State> {
    use smithay::input::pointer::{ButtonEvent, MotionEvent};
    let pointer = fixture.niri().seat.get_pointer().unwrap();
    pointer.motion(
        fixture.niri_state(),
        None,
        &MotionEvent {
            location: pos,
            serial: smithay::utils::Serial::from(601),
            time: smithay::backend::input::InputTime::now(),
        },
    );
    assert!(
        fixture.niri_state().try_start_spine_pointer_interaction(
            &pointer,
            0x110,
            smithay::utils::Serial::from(602)
        ),
        "pos={pos:?}, under={:?}, focus={:?}, admitted={}",
        fixture.niri().contents_under(pos).window,
        format!("{:?}", fixture.niri().keyboard_focus),
        fixture.niri_state().can_recall_sink(false)
    );
    pointer.button(
        fixture.niri_state(),
        &ButtonEvent {
            serial: smithay::utils::Serial::from(603),
            time: smithay::backend::input::InputTime::now(),
            button: 0x110,
            state: smithay::backend::input::ButtonState::Pressed,
        },
    );
    pointer
}

fn spine_test_pointer_motion(
    fixture: &mut Fixture,
    pointer: &smithay::input::pointer::PointerHandle<crate::niri::State>,
    pos: smithay::utils::Point<f64, smithay::utils::Logical>,
    framed: bool,
) {
    pointer.motion(
        fixture.niri_state(),
        None,
        &smithay::input::pointer::MotionEvent {
            location: pos,
            serial: smithay::utils::Serial::from(604),
            time: smithay::backend::input::InputTime::now(),
        },
    );
    if framed {
        pointer.frame(fixture.niri_state());
    }
}

fn spine_test_release(
    fixture: &mut Fixture,
    pointer: &smithay::input::pointer::PointerHandle<crate::niri::State>,
) {
    pointer.button(
        fixture.niri_state(),
        &smithay::input::pointer::ButtonEvent {
            serial: smithay::utils::Serial::from(605),
            time: smithay::backend::input::InputTime::now(),
            button: 0x110,
            state: smithay::backend::input::ButtonState::Released,
        },
    );
}

#[test]
fn disabling_spine_during_recognition_ends_its_grab_without_moving_restored_content() {
    let (mut fixture, client, _) = timed_shrink_fixture(2, false, 60000);
    let window = fixture.niri().layout.focus().unwrap().window.clone();
    let ticket = fixture
        .niri_state()
        .begin_shrink_preview(Some(ShrinkAxis::Horizontal))
        .unwrap()
        .ticket();
    fixture.niri_state().confirm_shrink_preview(ticket).unwrap();
    fixture.double_roundtrip(client);
    let pos = spine_test_point(&mut fixture, &window, (0.5, 0.5));
    let pointer = spine_test_press(&mut fixture, pos);
    let mut config = expand_test_config();
    config.shrink_expand.enabled = false;
    fixture.niri_state().reload_config(Ok(config));
    fixture.double_roundtrip(client);
    assert!(!fixture.niri().layout.is_spine(&window));
    spine_test_pointer_motion(
        &mut fixture,
        &pointer,
        pos + smithay::utils::Point::from((20., 0.)),
        true,
    );
    assert!(!pointer.is_grabbed());
    assert!(fixture.niri().layout.can_transfer_sink());
    assert!(!fixture
        .niri()
        .shrink_expand
        .records()
        .any(|r| r.kind == crate::shrink_expand::Kind::ExpandDirect));
}

#[test]
fn spine_touch_tap_drag_and_cancel_follow_existing_move_grab_lifecycle() {
    use crate::input::{move_grab::MoveGrab, AnyStartData};
    use smithay::input::touch::{DownEvent, GrabStartData, MotionEvent, UpEvent};
    for floating in [false, true] {
        for overview in [false, true] {
            for gesture in ["tap", "drag", "cancel"] {
                let (mut fixture, client, _) = timed_shrink_fixture(2, floating, 60000);
                let window = fixture.niri().layout.focus().unwrap().window.clone();
                let ticket = fixture
                    .niri_state()
                    .begin_shrink_preview(Some(ShrinkAxis::Horizontal))
                    .unwrap()
                    .ticket();
                fixture.niri_state().confirm_shrink_preview(ticket).unwrap();
                fixture.double_roundtrip(client);
                if overview {
                    fixture.niri().layout.toggle_overview();
                    fixture.niri_complete_animations();
                    fixture.double_roundtrip(client);
                }
                let pos = spine_test_point(&mut fixture, &window, (0.5, 0.5));
                let handle = fixture.niri().seat.add_touch();
                let slot = Some(0).into();
                let grab = MoveGrab::new(
                    fixture.niri_state(),
                    AnyStartData::Touch(GrabStartData {
                        focus: None,
                        slot,
                        location: pos,
                    }),
                    window.clone(),
                    false,
                    None,
                )
                .unwrap();
                handle.set_grab(
                    fixture.niri_state(),
                    grab,
                    smithay::utils::Serial::from(610),
                );
                handle.down(
                    fixture.niri_state(),
                    None,
                    &DownEvent {
                        slot,
                        location: pos,
                        serial: smithay::utils::Serial::from(611),
                        time: smithay::backend::input::InputTime::now(),
                    },
                );
                if gesture == "drag" {
                    handle.motion(
                        fixture.niri_state(),
                        None,
                        &MotionEvent {
                            slot,
                            location: pos + smithay::utils::Point::from((8., 0.)),
                            time: smithay::backend::input::InputTime::now(),
                        },
                    );
                }
                if gesture == "cancel" {
                    handle.cancel(fixture.niri_state());
                } else {
                    handle.up(
                        fixture.niri_state(),
                        &UpEvent {
                            slot,
                            serial: smithay::utils::Serial::from(612),
                            time: smithay::backend::input::InputTime::now(),
                        },
                    );
                }
                handle.frame(fixture.niri_state());
                fixture.double_roundtrip(client);
                assert!(!handle.is_grabbed());
                assert_eq!(fixture.niri().layout.is_spine(&window), gesture != "tap");
                assert!(fixture.niri().layout.can_transfer_sink());
            }
        }
    }
}

#[test]
fn spine_tablet_tip_and_proximity_out_distinguish_click_from_move_and_cancel() {
    use crate::input::{move_grab::MoveGrab, AnyStartData};
    use smithay::backend::input::{
        InputTime, TabletToolCapabilities, TabletToolDescriptor, TabletToolType,
    };
    use smithay::input::pointer::Focus;
    use smithay::input::tablet::tool::{
        DownEvent, GrabStartData, GrabTrigger, MotionEvent, ProximityInEvent, ProximityOutEvent,
        UpEvent,
    };
    use smithay::input::tablet::{TabletDescriptor, TabletSeatTrait};
    for floating in [false, true] {
        for overview in [false, true] {
            for gesture in ["tap", "drag", "cancel"] {
                let (mut fixture, client, _) = timed_shrink_fixture(2, floating, 60000);
                let window = fixture.niri().layout.focus().unwrap().window.clone();
                let ticket = fixture
                    .niri_state()
                    .begin_shrink_preview(Some(ShrinkAxis::Horizontal))
                    .unwrap()
                    .ticket();
                fixture.niri_state().confirm_shrink_preview(ticket).unwrap();
                fixture.double_roundtrip(client);
                if overview {
                    fixture.niri().layout.toggle_overview();
                    fixture.niri_complete_animations();
                    fixture.double_roundtrip(client);
                }
                let pos = spine_test_point(&mut fixture, &window, (0.5, 0.5));
                let seat = fixture.niri().seat.tablet_seat();
                let tablet = seat.add_tablet(&TabletDescriptor {
                    name: "spine-test".into(),
                    usb_id: None,
                    syspath: None,
                });
                let tool = seat.add_tool(&TabletToolDescriptor {
                    tool_type: TabletToolType::Pen,
                    hardware_serial: 1,
                    hardware_id_wacom: 1,
                    capabilities: TabletToolCapabilities::empty(),
                });
                tool.proximity_in(
                    fixture.niri_state(),
                    None,
                    tablet,
                    &ProximityInEvent {
                        location: pos,
                        axis: Default::default(),
                        serial: smithay::utils::Serial::from(620),
                        time: InputTime::now(),
                    },
                );
                let grab = MoveGrab::new(
                    fixture.niri_state(),
                    AnyStartData::TabletTool(GrabStartData {
                        focus: None,
                        trigger: GrabTrigger::Tip,
                        location: pos,
                    }),
                    window.clone(),
                    false,
                    None,
                )
                .unwrap();
                tool.set_grab(
                    fixture.niri_state(),
                    grab,
                    InputTime::now(),
                    smithay::utils::Serial::from(621),
                    Focus::Clear,
                );
                tool.down(
                    fixture.niri_state(),
                    &DownEvent {
                        serial: smithay::utils::Serial::from(622),
                        time: InputTime::now(),
                    },
                );
                if gesture == "drag" {
                    tool.motion(
                        fixture.niri_state(),
                        None,
                        &MotionEvent {
                            location: pos + smithay::utils::Point::from((8., 0.)),
                            serial: smithay::utils::Serial::from(623),
                            time: InputTime::now(),
                        },
                    );
                }
                if gesture == "cancel" {
                    tool.proximity_out(
                        fixture.niri_state(),
                        &ProximityOutEvent {
                            serial: smithay::utils::Serial::from(624),
                            time: InputTime::now(),
                        },
                    );
                } else {
                    tool.up(
                        fixture.niri_state(),
                        &UpEvent {
                            serial: smithay::utils::Serial::from(624),
                            time: InputTime::now(),
                        },
                    );
                }
                tool.frame(fixture.niri_state(), InputTime::now());
                fixture.double_roundtrip(client);
                assert!(!tool.is_grabbed());
                assert_eq!(fixture.niri().layout.is_spine(&window), gesture != "tap");
                assert!(fixture.niri().layout.can_transfer_sink());
            }
        }
    }
}

#[test]
fn spine_move_right_button_toggle_keeps_initiating_grab_and_normal_csd_pan_is_unchanged() {
    use crate::input::{move_grab::MoveGrab, AnyStartData};
    use smithay::input::pointer::{ButtonEvent, Focus, GrabStartData};
    let (mut fixture, client, _) = timed_shrink_fixture(2, false, 60000);
    let window = fixture.niri().layout.focus().unwrap().window.clone();
    let ticket = fixture
        .niri_state()
        .begin_shrink_preview(Some(ShrinkAxis::Horizontal))
        .unwrap()
        .ticket();
    fixture.niri_state().confirm_shrink_preview(ticket).unwrap();
    fixture.double_roundtrip(client);
    let pos = spine_test_point(&mut fixture, &window, (0.5, 0.5));
    let pointer = spine_test_press(&mut fixture, pos);
    for state in [
        smithay::backend::input::ButtonState::Pressed,
        smithay::backend::input::ButtonState::Released,
    ] {
        pointer.button(
            fixture.niri_state(),
            &ButtonEvent {
                serial: smithay::utils::Serial::from(630),
                time: smithay::backend::input::InputTime::now(),
                button: 0x111,
                state,
            },
        );
        assert!(pointer.is_grabbed());
    }
    spine_test_release(&mut fixture, &pointer);
    fixture.double_roundtrip(client);
    assert!(fixture.niri().layout.is_spine(&window));
    assert!(!pointer.is_grabbed());
    assert!(fixture
        .niri()
        .layout
        .active_workspace()
        .unwrap()
        .floating()
        .has_window(&window));
    let normal = fixture
        .niri()
        .layout
        .windows()
        .find(|(_, w)| w.window != window)
        .unwrap()
        .1
        .window
        .clone();
    let pos = (400., 300.).into();
    let grab = MoveGrab::new(
        fixture.niri_state(),
        AnyStartData::Pointer(GrabStartData {
            focus: None,
            button: 0x110,
            location: pos,
        }),
        normal,
        true,
        None,
    )
    .unwrap();
    pointer.set_grab(
        fixture.niri_state(),
        grab,
        smithay::utils::Serial::from(631),
        Focus::Clear,
    );
    pointer.button(
        fixture.niri_state(),
        &ButtonEvent {
            serial: smithay::utils::Serial::from(632),
            time: smithay::backend::input::InputTime::now(),
            button: 0x110,
            state: smithay::backend::input::ButtonState::Pressed,
        },
    );
    spine_test_pointer_motion(&mut fixture, &pointer, (420., 300.).into(), true);
    assert!(pointer
        .with_grab(|_, grab| grab
            .as_any()
            .downcast_ref::<MoveGrab>()
            .unwrap()
            .view_offset_output()
            .is_some())
        .unwrap());
    spine_test_release(&mut fixture, &pointer);
}

#[test]
fn spine_body_click_uses_real_hit_entry_and_expands_only_on_confirmed_release() {
    use crate::shrink_expand::{Kind, Phase};
    for floating in [false, true] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            for overview in [false, true] {
                let (mut fixture, client, surfaces) = timed_shrink_fixture(2, floating, 60000);
                let window = fixture.niri().layout.focus().unwrap().window.clone();
                let ticket = fixture
                    .niri_state()
                    .begin_shrink_preview(Some(axis))
                    .unwrap()
                    .ticket();
                fixture.niri_state().confirm_shrink_preview(ticket).unwrap();
                fixture.niri_state().reload_config(Ok(expand_test_config()));
                fixture.double_roundtrip(client);
                if overview {
                    fixture.niri().layout.toggle_overview();
                    fixture.niri_complete_animations();
                    fixture.double_roundtrip(client);
                }
                let pos = spine_test_point(&mut fixture, &window, (0.5, 0.5));
                let hit = fixture.niri_state().spine_interaction_hit(pos);
                assert!(hit.is_some(), "floating={floating} axis={axis:?} overview={overview} pos={pos:?}, under={:?}, focus={:?}, admitted={}", fixture.niri().contents_under(pos).window, format!("{:?}", fixture.niri().keyboard_focus), fixture.niri_state().can_recall_sink(false));
                assert!(hit.unwrap().1.is_empty());
                let before = fixture.niri().keyboard_focus.surface().cloned();
                let pointer = spine_test_press(&mut fixture, pos);
                fixture.double_roundtrip(client);
                assert!(pointer.is_grabbed());
                assert!(pointer.current_focus().is_none());
                assert!(fixture.niri().layout.is_spine(&window));
                assert_eq!(fixture.niri().keyboard_focus.surface().cloned(), before);
                // A small final motion without frame is still a click.
                spine_test_pointer_motion(
                    &mut fixture,
                    &pointer,
                    pos + smithay::utils::Point::from((7., 0.)),
                    false,
                );
                spine_test_release(&mut fixture, &pointer);
                fixture.double_roundtrip(client);
                assert!(!pointer.is_grabbed());
                assert!(!fixture.niri().layout.is_spine(&window));
                assert!(!fixture.niri().layout.is_overview_open());
                assert_eq!(fixture.niri().layout.windows().count(), 2);
                let record = fixture
                    .niri()
                    .shrink_expand
                    .records()
                    .find(|r| r.kind == Kind::ExpandDirect)
                    .unwrap()
                    .clone();
                assert_eq!(record.phase, Phase::Submitted);
                let target = fixture.client(client).window(surfaces.last().unwrap());
                let size = target.configures_received.last().unwrap().1.size;
                target.set_size(size.0 as u16, size.1 as u16);
                target.ack_last_and_commit();
                fixture.double_roundtrip(client);
                assert!(fixture
                    .niri()
                    .shrink_expand
                    .records()
                    .any(|r| r.id == record.id && r.phase == Phase::Settled));
                assert!(fixture
                    .niri()
                    .layout
                    .focus()
                    .is_some_and(|w| w.window == window));
            }
        }
    }
}

#[test]
fn spine_move_recognition_short_return_and_unframed_release_never_click_expand() {
    for floating in [false, true] {
        for framed in [false, true] {
            for distance in [8., 20., 300.] {
                let (mut fixture, client, _) = timed_shrink_fixture(2, floating, 60000);
                let window = fixture.niri().layout.focus().unwrap().window.clone();
                let ticket = fixture
                    .niri_state()
                    .begin_shrink_preview(Some(ShrinkAxis::Horizontal))
                    .unwrap()
                    .ticket();
                fixture.niri_state().confirm_shrink_preview(ticket).unwrap();
                fixture.double_roundtrip(client);
                let pos = spine_test_point(&mut fixture, &window, (0.5, 0.5));
                let pointer = spine_test_press(&mut fixture, pos);
                spine_test_pointer_motion(
                    &mut fixture,
                    &pointer,
                    pos + smithay::utils::Point::from((distance, 0.)),
                    framed,
                );
                if framed {
                    assert!(!fixture.niri().layout.can_transfer_sink());
                    spine_test_pointer_motion(&mut fixture, &pointer, pos, true);
                }
                spine_test_release(&mut fixture, &pointer);
                fixture.double_roundtrip(client);
                assert!(!pointer.is_grabbed());
                assert!(fixture.niri().layout.can_transfer_sink());
                assert!(fixture.niri().layout.is_spine(&window));
                assert!(!fixture
                    .niri()
                    .shrink_expand
                    .records()
                    .any(|r| r.kind == crate::shrink_expand::Kind::ExpandDirect));
                assert_eq!(fixture.niri().layout.windows().count(), 2);
            }
        }
    }
}

#[test]
fn spine_click_forced_ungrab_opposite_preview_and_changed_scope_do_not_expand() {
    for cancel in 0..4 {
        let (mut fixture, client, _) = timed_shrink_fixture(2, false, 60000);
        let window = fixture.niri().layout.focus().unwrap().window.clone();
        let ticket = fixture
            .niri_state()
            .begin_shrink_preview(Some(ShrinkAxis::Horizontal))
            .unwrap()
            .ticket();
        fixture.niri_state().confirm_shrink_preview(ticket).unwrap();
        fixture.double_roundtrip(client);
        if cancel == 1 {
            fixture.niri_state().begin_shrink_preview(None).unwrap();
        }
        let pos = spine_test_point(&mut fixture, &window, (0.5, 0.5));
        let pointer = spine_test_press(&mut fixture, pos);
        if cancel == 0 {
            pointer.unset_grab(
                fixture.niri_state(),
                smithay::utils::Serial::from(606),
                smithay::backend::input::InputTime::now(),
            );
        } else {
            spine_test_release(&mut fixture, &pointer);
            if cancel == 2 {
                fixture.niri().layout.move_to_workspace(
                    Some(&window),
                    1,
                    crate::layout::ActivateWindow::No,
                );
            }
            if cancel == 3 {
                fixture.niri().layout.toggle_overview();
            }
        }
        fixture.double_roundtrip(client);
        assert!(!pointer.is_grabbed());
        assert!(fixture.niri().layout.is_spine(&window));
        assert!(!fixture
            .niri()
            .shrink_expand
            .records()
            .any(|r| r.kind == crate::shrink_expand::Kind::ExpandDirect));
        assert!(fixture.niri().shrink_expand.preview().is_none());
    }
}

#[test]
fn spine_edge_hit_starts_native_resize_and_overview_delta_uses_layout_coordinates() {
    use crate::input::resize_grab::ResizeGrab;
    for floating in [false, true] {
        for overview in [false, true] {
            let (mut fixture, client, _) = timed_shrink_fixture(2, floating, 60000);
            let window = fixture.niri().layout.focus().unwrap().window.clone();
            let ticket = fixture
                .niri_state()
                .begin_shrink_preview(Some(ShrinkAxis::Horizontal))
                .unwrap()
                .ticket();
            fixture.niri_state().confirm_shrink_preview(ticket).unwrap();
            fixture.double_roundtrip(client);
            if overview {
                fixture.niri().layout.toggle_overview();
                fixture.niri_complete_animations();
                fixture.double_roundtrip(client);
            }
            let size = fixture
                .niri()
                .layout
                .workspaces()
                .flat_map(|(_, _, ws)| ws.tiles())
                .find(|t| t.window().window == window)
                .unwrap()
                .tile_size();
            let pos = spine_test_point(&mut fixture, &window, ((size.w - 1.) / size.w, 0.5));
            let hit = fixture.niri_state().spine_interaction_hit(pos);
            assert!(
                hit.is_some(),
                "floating={floating} overview={overview} pos={pos:?}, under={:?}, focus={:?}",
                fixture.niri().contents_under(pos).window,
                format!("{:?}", fixture.niri().keyboard_focus)
            );
            assert!(!hit.unwrap().1.is_empty());
            let zoom = fixture.niri().layout.overview_zoom();
            let pointer = spine_test_press(&mut fixture, pos);
            assert!(pointer
                .with_grab(|_, grab| grab.is::<ResizeGrab>())
                .unwrap());
            spine_test_pointer_motion(
                &mut fixture,
                &pointer,
                pos + smithay::utils::Point::from((10. * zoom, 0.)),
                true,
            );
            fixture.double_roundtrip(client);
            assert!(!fixture.niri().layout.is_spine(&window));
            let raw = fixture
                .niri()
                .layout
                .spatial_resize_intent(&window, (10., 0.).into())
                .unwrap();
            assert!((raw.requested.w - size.w - 10.).abs() < 1e-6);
            spine_test_release(&mut fixture, &pointer);
            fixture.double_roundtrip(client);
            assert!(!pointer.is_grabbed());
        }
    }
}

#[test]
fn matching_restore_release_keeps_a_foreign_staged_transaction() {
    use crate::utils::transaction::Transaction;
    let (mut fixture, _, _) = timed_shrink_fixture(1, true, 60000);
    let id = fixture.niri().layout.focus().unwrap().window.clone();
    let owned = Transaction::new();
    let foreign = Transaction::new();
    let mapped = fixture
        .niri()
        .layout
        .workspaces_mut()
        .flat_map(|ws| ws.tiles_mut())
        .find(|tile| tile.window().window == id)
        .unwrap()
        .window_mut();
    mapped.request_restore_size_once((720, 400).into(), owned.clone());
    assert!(!owned.is_last());
    mapped.release_matching_normal_restore_transaction(&foreign);
    assert!(
        !owned.is_last(),
        "another restore's batch must not release this staged transaction"
    );
    mapped.release_matching_normal_restore_transaction(&owned);
    assert!(owned.is_last());
}

#[test]
fn floating_spine_resize_preserves_client_choice_extent_while_restore_is_pending() {
    use crate::utils::ResizeEdge;
    let (mut fixture, client, _) = timed_shrink_fixture(2, true, 60000);
    let window = fixture.niri().layout.focus().unwrap().window.clone();
    let ticket = fixture
        .niri_state()
        .begin_shrink_preview(Some(ShrinkAxis::Horizontal))
        .unwrap()
        .ticket();
    fixture.niri_state().confirm_shrink_preview(ticket).unwrap();
    fixture.double_roundtrip(client);
    let (_, pointer) =
        spatial_test_pointer_for_window(&mut fixture, ResizeEdge::RIGHT, window.clone());
    spatial_test_motion(&mut fixture, &pointer, (80., 0.));
    fixture.double_roundtrip(client);
    let before = fixture
        .niri()
        .layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles())
        .find(|tile| tile.window().window == window)
        .unwrap()
        .tile_size();
    spatial_test_motion(&mut fixture, &pointer, (100., 0.));
    fixture.double_roundtrip(client);
    let tile = fixture
        .niri()
        .layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles())
        .find(|tile| tile.window().window == window)
        .unwrap();
    assert!(tile.normal_restore_input_blocked());
    assert_eq!(
        tile.window().requested_size().unwrap().h,
        tile.window().size().h
    );
    assert_eq!(tile.tile_size().h, before.h);
    assert!(before.h > 0.);
    assert_eq!(tile.tile_size().w - before.w, 20.);
    assert!(pointer.is_grabbed());
    pointer.unset_grab(
        fixture.niri_state(),
        smithay::utils::Serial::from(460),
        smithay::backend::input::InputTime::now(),
    );
}

#[test]
fn disable_runtime_ends_book_move_through_existing_pointer_grab_but_keeps_normal_move() {
    use crate::input::{move_grab::MoveGrab, AnyStartData};
    use smithay::input::pointer::{ButtonEvent, Focus, GrabStartData};
    for spine in [false, true] {
        for floating in [false, true] {
            for detach in [false, true] {
                let (mut fixture, client, _) = timed_shrink_fixture(2, floating, 60000);
                let window = fixture.niri().layout.focus().unwrap().window.clone();
                if spine {
                    let ticket = fixture
                        .niri_state()
                        .begin_shrink_preview(Some(ShrinkAxis::Horizontal))
                        .unwrap()
                        .ticket();
                    fixture.niri_state().confirm_shrink_preview(ticket).unwrap();
                    fixture.double_roundtrip(client);
                }
                let pointer = fixture.niri().seat.get_pointer().unwrap();
                pointer.button(
                    fixture.niri_state(),
                    &ButtonEvent {
                        serial: smithay::utils::Serial::from(500),
                        time: smithay::backend::input::InputTime::now(),
                        button: 0x110,
                        state: smithay::backend::input::ButtonState::Pressed,
                    },
                );
                let grab = MoveGrab::new(
                    fixture.niri_state(),
                    AnyStartData::Pointer(GrabStartData {
                        focus: None,
                        button: 0x110,
                        location: (400., 300.).into(),
                    }),
                    window.clone(),
                    false,
                    None,
                )
                .unwrap();
                pointer.set_grab(
                    fixture.niri_state(),
                    grab,
                    smithay::utils::Serial::from(501),
                    Focus::Clear,
                );
                let delta = if detach { (300., 0.) } else { (20., 0.) };
                spatial_test_motion(&mut fixture, &pointer, delta);
                fixture.double_roundtrip(client);
                assert!(pointer.is_grabbed());
                assert!(!fixture.niri().layout.can_transfer_sink());
                let mut config = expand_test_config();
                config.shrink_expand.enabled = false;
                fixture.niri_state().reload_config(Ok(config));
                fixture.double_roundtrip(client);
                assert_eq!(fixture.niri().layout.can_transfer_sink(), spine);
                assert!(!fixture.niri().layout.is_spine(&window));
                spatial_test_motion(&mut fixture, &pointer, (delta.0 + 10., 0.));
                fixture.double_roundtrip(client);
                assert_eq!(pointer.is_grabbed(), !spine);
                assert_eq!(fixture.niri().layout.windows().count(), 2);
                if !spine {
                    pointer.unset_grab(
                        fixture.niri_state(),
                        smithay::utils::Serial::from(502),
                        smithay::backend::input::InputTime::now(),
                    );
                    fixture.double_roundtrip(client);
                }
            }
        }
    }
}

#[test]
fn disable_runtime_restores_all_workspaces_tabs_and_floats_with_original_deadline() {
    use crate::layout::ActivateWindow;
    use crate::shrink_expand::{Kind, Phase};
    for all_books in [false, true] {
        for no_outputs in [false, true] {
            let (mut fixture, client, surfaces) = timed_shrink_fixture(6, false, 60000);
            let mut windows = fixture
                .niri()
                .layout
                .windows()
                .map(|(_, w)| (w.id().get(), w.window.clone()))
                .collect::<Vec<_>>();
            windows.sort_by_key(|(id, _)| *id);
            for i in [0, 3] {
                fixture
                    .niri()
                    .layout
                    .set_window_floating(Some(&windows[i].1), true);
            }
            fixture
                .niri()
                .layout
                .consume_or_expel_window_left(Some(&windows[2].1));
            fixture.niri().layout.activate_window(&windows[2].1);
            fixture
                .niri()
                .layout
                .set_column_display(niri_ipc::ColumnDisplay::Tabbed);
            fixture
                .niri()
                .layout
                .move_to_workspace(Some(&windows[4].1), 1, ActivateWindow::No);
            fixture.add_output(2, (1024, 768));
            let output2 = fixture
                .niri()
                .layout
                .outputs()
                .find(|output| output.name() == "headless-2")
                .unwrap()
                .clone();
            fixture.niri().layout.move_to_output(
                Some(&windows[5].1),
                &output2,
                None,
                ActivateWindow::No,
            );
            fixture.niri().layout.switch_workspace(0);
            fixture.niri().layout.activate_window(&windows[2].1);
            let targets = windows
                .iter()
                .enumerate()
                .filter(|(i, _)| all_books || *i != 2)
                .map(|(_, pair)| pair.clone())
                .collect::<Vec<_>>();
            for (i, (_, id)) in targets.iter().enumerate() {
                let ws = fixture
                    .niri()
                    .layout
                    .workspaces_mut()
                    .find(|ws| ws.has_window(id))
                    .unwrap();
                assert!(ws
                    .tiles_mut()
                    .find(|t| &t.window().window == id)
                    .unwrap()
                    .set_spine(
                        if i % 2 == 0 {
                            ShrinkAxis::Horizontal
                        } else {
                            ShrinkAxis::Vertical
                        },
                        32.
                    ));
                ws.update_window(id, None);
            }
            fixture.double_roundtrip(client);
            if no_outputs {
                let outputs = fixture.niri().layout.outputs().cloned().collect::<Vec<_>>();
                for output in outputs {
                    fixture.niri().remove_output(&output);
                }
                fixture.double_roundtrip(client);
            }
            let focus = fixture.niri().keyboard_focus.surface().cloned();
            assert_eq!(focus.is_none(), all_books || no_outputs);
            let slots = fixture
                .niri()
                .layout
                .workspaces()
                .filter(|(_, _, ws)| ws.windows().next().is_some())
                .map(|(_, _, ws)| {
                    (
                        ws.id(),
                        ws.tiles()
                            .map(|t| {
                                (
                                    t.window().id().get(),
                                    ws.floating().has_window(&t.window().window),
                                )
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>();
            let mut config = expand_test_config();
            config.shrink_expand.enabled = false;
            fixture.niri_state().reload_config(Ok(config));
            fixture.double_roundtrip(client);
            assert!(fixture
                .niri()
                .layout
                .workspaces()
                .flat_map(|(_, _, ws)| ws.tiles())
                .all(|tile| !tile.is_spine()));
            assert_eq!(fixture.niri().keyboard_focus.surface().cloned(), focus);
            assert_eq!(
                fixture
                    .niri()
                    .layout
                    .workspaces()
                    .filter(|(_, _, ws)| ws.windows().next().is_some())
                    .map(|(_, _, ws)| (
                        ws.id(),
                        ws.tiles()
                            .map(|t| (
                                t.window().id().get(),
                                ws.floating().has_window(&t.window().window)
                            ))
                            .collect::<Vec<_>>()
                    ))
                    .collect::<Vec<_>>(),
                slots
            );
            let records = fixture
                .niri()
                .shrink_expand
                .records()
                .filter(|r| r.kind == Kind::ExpandAll)
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(records.len(), 3);
            assert!(records.iter().all(|r| r.phase == Phase::Submitted));
            let mut actual = records
                .iter()
                .flat_map(|r| r.targets.clone())
                .collect::<Vec<_>>();
            actual.sort();
            let mut expected = targets.iter().map(|(id, _)| *id).collect::<Vec<_>>();
            expected.sort();
            assert_eq!(actual, expected);
            let deadlines = fixture
                .niri()
                .layout
                .windows()
                .filter(|(_, w)| actual.contains(&w.id().get()))
                .filter_map(|(_, w)| w.normal_restore_deadline())
                .collect::<Vec<_>>();
            assert!(!deadlines.is_empty());
            assert!(fixture
                .niri()
                .layout
                .windows()
                .filter(|(_, w)| actual.contains(&w.id().get()))
                .all(|(_, w)| w.normal_restore_deadline().is_some() || w.normal_restore_ready()));
            assert!(deadlines.iter().all(|d| *d == deadlines[0]));
            let counts = surfaces
                .iter()
                .map(|surface| {
                    fixture
                        .client(client)
                        .window(surface)
                        .configures_received
                        .len()
                })
                .collect::<Vec<_>>();
            let mut config = expand_test_config();
            config.shrink_expand.enabled = false;
            fixture.niri_state().reload_config(Ok(config));
            fixture.double_roundtrip(client);
            assert_eq!(
                fixture
                    .niri()
                    .shrink_expand
                    .records()
                    .filter(|r| r.kind == Kind::ExpandAll)
                    .count(),
                3
            );
            assert_eq!(
                fixture
                    .niri()
                    .layout
                    .windows()
                    .filter(|(_, w)| actual.contains(&w.id().get()))
                    .filter_map(|(_, w)| w.normal_restore_deadline())
                    .collect::<Vec<_>>(),
                deadlines
            );
            assert_eq!(
                surfaces
                    .iter()
                    .map(|surface| fixture
                        .client(client)
                        .window(surface)
                        .configures_received
                        .len())
                    .collect::<Vec<_>>(),
                counts,
                "same-size reload must not continually publish another restore configure"
            );
            // The only target on output 2 closes while this global batch is pending.
            let closed = fixture.client(client).window(surfaces.last().unwrap());
            closed.xdg_toplevel.destroy();
            closed.xdg_surface.destroy();
            closed.surface.destroy();
            fixture.double_roundtrip(client);
            assert_eq!(fixture.niri().layout.windows().count(), 5);
            assert!(fixture
                .niri()
                .shrink_expand
                .records()
                .any(|r| r.kind == Kind::ExpandAll
                    && r.targets == vec![windows[5].0]
                    && r.reason == Some(crate::shrink_expand::FinishReason::TargetsRemoved)));
            for surface in &surfaces[..5] {
                let window = fixture.client(client).window(surface);
                let size = window.configures_received.last().unwrap().1.size;
                window.set_size(size.0 as u16, size.1 as u16);
                window.ack_last_and_commit();
            }
            // Commit-blocker notifications are asynchronous and can need more than two
            // calloop cycles when several workspace transactions settle together.
            for _ in 0..4 {
                fixture.double_roundtrip(client);
                if fixture
                    .niri()
                    .shrink_expand
                    .records()
                    .filter(|r| r.kind == Kind::ExpandAll)
                    .all(|r| r.phase == Phase::Settled)
                {
                    break;
                }
            }
            assert_eq!(fixture.niri().keyboard_focus.surface().cloned(), focus);
            let records = fixture
                .niri()
                .shrink_expand
                .records()
                .filter(|r| r.kind == Kind::ExpandAll)
                .cloned()
                .collect::<Vec<_>>();
            let states = fixture
                .niri()
                .layout
                .windows()
                .map(|(_, w)| {
                    (
                        w.id().get(),
                        w.size(),
                        w.requested_size(),
                        w.normal_restore_ready(),
                        w.normal_restore_deadline(),
                    )
                })
                .collect::<Vec<_>>();
            assert!(records.iter().all(|r| r.phase == Phase::Settled), "all_books={all_books} no_outputs={no_outputs}, records={records:?}, states={states:?}");
            assert!(records
                .iter()
                .filter(|r| r.targets != vec![windows[5].0])
                .all(|r| r.reason == Some(crate::shrink_expand::FinishReason::Ready)));
        }
    }
}

#[test]
fn disable_runtime_rebases_book_resize_and_keeps_the_native_pointer_grab() {
    use crate::utils::ResizeEdge;
    for floating in [false, true] {
        for framed in [false, true] {
            let (mut fixture, client, _) = timed_shrink_fixture(2, floating, 60000);
            let window = fixture.niri().layout.focus().unwrap().window.clone();
            let ticket = fixture
                .niri_state()
                .begin_shrink_preview(Some(ShrinkAxis::Horizontal))
                .unwrap()
                .ticket();
            fixture.niri_state().confirm_shrink_preview(ticket).unwrap();
            fixture.double_roundtrip(client);
            let (_, pointer) =
                spatial_test_pointer_for_window(&mut fixture, ResizeEdge::BOTTOM, window.clone());
            let delta = if framed { (0., 40.) } else { (0., 0.) };
            if framed {
                spatial_test_motion(&mut fixture, &pointer, delta);
                fixture.double_roundtrip(client);
            }
            let focus = fixture.niri().keyboard_focus.surface().cloned();
            let mut config = expand_test_config();
            config.shrink_expand.enabled = false;
            fixture.niri_state().reload_config(Ok(config));
            fixture.double_roundtrip(client);
            assert!(!fixture.niri().layout.is_spine(&window));
            assert!(pointer.is_grabbed());
            assert_eq!(fixture.niri().keyboard_focus.surface().cloned(), focus);
            let before = fixture
                .niri()
                .layout
                .spatial_resize_intent(&window, delta.into())
                .unwrap();
            spatial_test_motion(&mut fixture, &pointer, (0., delta.1 + 20.));
            fixture.double_roundtrip(client);
            let after = fixture
                .niri()
                .layout
                .spatial_resize_intent(&window, (0., delta.1 + 20.).into())
                .unwrap();
            assert_eq!(after.requested.h - before.requested.h, 20.);
            assert!(pointer.is_grabbed());
            assert!(fixture.niri().shrink_expand.preview().is_none());
            pointer.unset_grab(
                fixture.niri_state(),
                smithay::utils::Serial::from(450),
                smithay::backend::input::InputTime::now(),
            );
            fixture.double_roundtrip(client);
            assert!(!pointer.is_grabbed());
        }
    }
}

#[test]
fn spine_resize_expands_rebases_same_grab_and_can_shrink_again() {
    use crate::shrink_expand::{Kind, Phase};
    use crate::utils::ResizeEdge;
    use smithay::input::pointer::ButtonEvent;
    for floating in [false, true] {
        for (axis, edges, grow, further, fold) in [
            (
                ShrinkAxis::Horizontal,
                ResizeEdge::RIGHT,
                (180., 0.),
                (200., 0.),
                (-300., 0.),
            ),
            (
                ShrinkAxis::Horizontal,
                ResizeEdge::LEFT,
                (-180., 0.),
                (-200., 0.),
                (300., 0.),
            ),
            (
                ShrinkAxis::Vertical,
                ResizeEdge::BOTTOM,
                (0., 180.),
                (0., 200.),
                (0., -300.),
            ),
        ] {
            let (mut fixture, client, surfaces) = timed_shrink_fixture(2, floating, 60000);
            let surface = surfaces.last().unwrap();
            fixture
                .client(client)
                .window(surface)
                .xdg_toplevel
                .set_min_size(150, 120);
            fixture.client(client).window(surface).commit();
            fixture.double_roundtrip(client);
            let window = fixture.niri().layout.focus().unwrap().window.clone();
            let ticket = fixture
                .niri_state()
                .begin_shrink_preview(Some(axis))
                .unwrap()
                .ticket();
            fixture.niri_state().confirm_shrink_preview(ticket).unwrap();
            fixture.double_roundtrip(client);
            assert!(fixture.niri().layout.is_spine(&window));
            let (window, pointer) = spatial_test_pointer_for_window(&mut fixture, edges, window);
            spatial_test_motion(&mut fixture, &pointer, grow);
            fixture.double_roundtrip(client);
            assert!(
                !fixture.niri().layout.is_spine(&window),
                "floating={floating} axis={axis:?}"
            );
            assert!(pointer.is_grabbed());
            assert!(fixture.niri().shrink_expand.preview().is_none());
            assert!(fixture
                .niri()
                .shrink_expand
                .records()
                .any(|r| r.kind == Kind::ExpandDirect && r.phase == Phase::Submitted));
            assert!(fixture.niri().shrink_expand_focus.is_some());
            let before = fixture
                .niri()
                .layout
                .spatial_resize_intent(&window, grow.into())
                .unwrap();
            let deadline = fixture
                .niri()
                .layout
                .windows()
                .find(|(_, w)| w.window == window)
                .unwrap()
                .1
                .normal_restore_deadline();
            spatial_test_motion(&mut fixture, &pointer, further);
            fixture.double_roundtrip(client);
            let after = fixture
                .niri()
                .layout
                .spatial_resize_intent(&window, further.into())
                .unwrap();
            let difference = match axis {
                ShrinkAxis::Horizontal => after.requested.w - before.requested.w,
                ShrinkAxis::Vertical => after.requested.h - before.requested.h,
            };
            assert_eq!(difference, 20.);
            assert_eq!(
                fixture
                    .niri()
                    .layout
                    .windows()
                    .find(|(_, w)| w.window == window)
                    .unwrap()
                    .1
                    .normal_restore_deadline(),
                deadline
            );
            let client_window = fixture.client(client).window(surface);
            let size = client_window.configures_received.last().unwrap().1.size;
            client_window.set_size(size.0 as u16, size.1 as u16);
            client_window.ack_last_and_commit();
            fixture.double_roundtrip(client);
            assert!(fixture
                .niri()
                .layout
                .focus()
                .is_some_and(|w| w.window == window));
            assert!(pointer.is_grabbed());
            spatial_test_motion(&mut fixture, &pointer, fold);
            fixture.double_roundtrip(client);
            assert!(
                fixture.niri().shrink_expand.preview().is_some(),
                "floating={floating} axis={axis:?}"
            );
            pointer.button(
                fixture.niri_state(),
                &ButtonEvent {
                    serial: smithay::utils::Serial::from(403),
                    time: smithay::backend::input::InputTime::now(),
                    button: 0x110,
                    state: smithay::backend::input::ButtonState::Released,
                },
            );
            fixture.double_roundtrip(client);
            assert!(!pointer.is_grabbed());
            assert!(fixture.niri().layout.is_spine(&window));
            assert!(fixture
                .niri()
                .shrink_expand
                .records()
                .any(|r| r.kind == Kind::ShrinkSpatial(axis) && r.phase == Phase::Settled));
        }
    }
}

#[test]
fn spine_resize_free_axis_stays_collapsed_and_last_motion_can_expand() {
    use crate::utils::ResizeEdge;
    use smithay::input::pointer::{ButtonEvent, MotionEvent};
    for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
        let (mut fixture, client, _) = timed_shrink_fixture(2, true, 60000);
        let window = fixture.niri().layout.focus().unwrap().window.clone();
        let ticket = fixture
            .niri_state()
            .begin_shrink_preview(Some(axis))
            .unwrap()
            .ticket();
        fixture.niri_state().confirm_shrink_preview(ticket).unwrap();
        fixture.double_roundtrip(client);
        let (window, pointer) = spatial_test_pointer_for_window(
            &mut fixture,
            ResizeEdge::RIGHT | ResizeEdge::BOTTOM,
            window,
        );
        let free = match axis {
            ShrinkAxis::Horizontal => (0., 40.),
            ShrinkAxis::Vertical => (40., 0.),
        };
        spatial_test_motion(&mut fixture, &pointer, free);
        fixture.double_roundtrip(client);
        assert!(fixture.niri().layout.is_spine(&window));
        assert!(fixture.niri().shrink_expand.preview().is_none());
        pointer.motion(
            fixture.niri_state(),
            None,
            &MotionEvent {
                location: (580., 480.).into(),
                serial: smithay::utils::Serial::from(404),
                time: smithay::backend::input::InputTime::now(),
            },
        );
        pointer.button(
            fixture.niri_state(),
            &ButtonEvent {
                serial: smithay::utils::Serial::from(405),
                time: smithay::backend::input::InputTime::now(),
                button: 0x110,
                state: smithay::backend::input::ButtonState::Released,
            },
        );
        fixture.double_roundtrip(client);
        assert!(!pointer.is_grabbed());
        assert!(!fixture.niri().layout.is_spine(&window));
    }
}

#[test]
fn spatial_real_pointer_enters_exits_and_release_submits_without_timer() {
    use crate::shrink_expand::{Kind, Phase};
    use crate::utils::ResizeEdge;
    use smithay::input::pointer::ButtonEvent;
    for floating in [false, true] {
        for (edges, axis, delta) in [
            (ResizeEdge::RIGHT, ShrinkAxis::Horizontal, (-140., 0.)),
            (ResizeEdge::LEFT, ShrinkAxis::Horizontal, (140., 0.)),
            (ResizeEdge::BOTTOM, ShrinkAxis::Vertical, (0., -70.)),
        ] {
            let (mut fixture, client, surfaces) = timed_shrink_fixture(2, floating, 60000);
            fixture
                .client(client)
                .window(surfaces.last().unwrap())
                .xdg_toplevel
                .set_min_size(150, 120);
            fixture
                .client(client)
                .window(surfaces.last().unwrap())
                .commit();
            fixture.double_roundtrip(client);
            let (window, pointer) = spatial_test_pointer(&mut fixture, edges);
            spatial_test_motion(&mut fixture, &pointer, delta);
            fixture.double_roundtrip(client);
            let first = fixture
                .niri()
                .shrink_expand
                .preview()
                .unwrap()
                .record
                .clone();
            assert_eq!(first.kind, Kind::ShrinkSpatial(axis));
            assert!(fixture
                .niri()
                .shrink_expand
                .preview()
                .unwrap()
                .deadline
                .is_none());
            assert!(fixture.niri().shrink_preview_timer.is_none());
            assert!(pointer.is_grabbed());
            assert!(!fixture.niri().layout.is_spine(&window));
            spatial_test_motion(&mut fixture, &pointer, (0., 0.));
            fixture.double_roundtrip(client);
            assert!(fixture.niri().shrink_expand.preview().is_none());
            assert!(pointer.is_grabbed());
            assert!(fixture
                .niri()
                .shrink_expand
                .records()
                .any(|r| r.id == first.id && r.phase == Phase::Cancelled));
            spatial_test_motion(&mut fixture, &pointer, delta);
            fixture.double_roundtrip(client);
            assert!(fixture.niri().shrink_expand.preview().is_some());
            pointer.button(
                fixture.niri_state(),
                &ButtonEvent {
                    serial: smithay::utils::Serial::from(403),
                    time: smithay::backend::input::InputTime::now(),
                    button: 0x110,
                    state: smithay::backend::input::ButtonState::Released,
                },
            );
            fixture.double_roundtrip(client);
            assert!(!pointer.is_grabbed());
            assert!(fixture.niri().layout.is_spine(&window));
            assert!(fixture.niri().shrink_expand.preview().is_none());
            assert!(fixture.niri().shrink_spatial_session.is_none());
            assert!(fixture
                .niri()
                .layout
                .focus()
                .is_some_and(|w| w.window != window));
        }
    }
}

#[test]
fn spatial_cancel_inhibits_current_zone_and_forced_ungrab_never_folds() {
    use crate::utils::ResizeEdge;
    use smithay::utils::Serial;
    for mode in ["abort", "command", "escape"] {
        let (mut fixture, client, surfaces) = timed_shrink_fixture(2, true, 60000);
        fixture
            .client(client)
            .window(surfaces.last().unwrap())
            .xdg_toplevel
            .set_min_size(150, 120);
        fixture
            .client(client)
            .window(surfaces.last().unwrap())
            .commit();
        fixture.double_roundtrip(client);
        let (window, pointer) = spatial_test_pointer(&mut fixture, ResizeEdge::RIGHT);
        spatial_test_motion(&mut fixture, &pointer, (-140., 0.));
        fixture.double_roundtrip(client);
        assert!(fixture.niri().shrink_expand.preview().is_some());
        if mode == "abort" {
            pointer.unset_grab(
                fixture.niri_state(),
                Serial::from(404),
                smithay::backend::input::InputTime::now(),
            );
        } else {
            if mode == "command" {
                fixture
                    .niri_state()
                    .do_action(niri_config::Action::CancelShrinkExpand, false);
            } else {
                use super::test_input_backend::{TestInputBackend, TestKeyboardKeyEvent};
                use smithay::backend::input::{InputEvent, InputTime, KeyState};
                let keyboard = fixture.niri().seat.get_keyboard().unwrap();
                let code = keyboard.with_xkb_state(fixture.niri_state(), |xkb| {
                    unsafe { xkb.xkb().lock().unwrap().keymap() }
                        .key_by_name("ESC")
                        .unwrap()
                });
                for state in [KeyState::Pressed, KeyState::Released] {
                    fixture.niri_state().process_input_event(
                        InputEvent::<TestInputBackend>::Keyboard {
                            event: TestKeyboardKeyEvent {
                                time: InputTime::from_micros(0),
                                code,
                                state,
                                count: 1,
                            },
                        },
                    );
                }
                assert!(fixture.niri().suppressed_keys.is_empty());
            }
            assert!(pointer.is_grabbed());
            spatial_test_motion(&mut fixture, &pointer, (-145., 0.));
            fixture.double_roundtrip(client);
            assert!(fixture.niri().shrink_expand.preview().is_none());
            pointer.button(
                fixture.niri_state(),
                &smithay::input::pointer::ButtonEvent {
                    serial: Serial::from(405),
                    time: smithay::backend::input::InputTime::now(),
                    button: 0x110,
                    state: smithay::backend::input::ButtonState::Released,
                },
            );
        }
        fixture.double_roundtrip(client);
        assert!(!fixture.niri().layout.is_spine(&window));
        assert!(fixture.niri().shrink_spatial_session.is_none());
        assert!(fixture.niri().shrink_expand.preview().is_none());
    }
}

#[test]
fn spatial_release_flushes_only_a_candidate_and_preserves_ordinary_frame_batching() {
    use crate::utils::ResizeEdge;
    use smithay::backend::input::{ButtonState, InputTime};
    use smithay::input::pointer::{ButtonEvent, MotionEvent};
    use smithay::utils::Serial;
    for (enabled, delta, fold) in [
        (false, -140., false),
        (true, -20., false),
        (true, -140., true),
    ] {
        let (mut fixture, client, surfaces) = timed_shrink_fixture(2, true, 60000);
        fixture
            .client(client)
            .window(surfaces.last().unwrap())
            .xdg_toplevel
            .set_min_size(150, 120);
        fixture
            .client(client)
            .window(surfaces.last().unwrap())
            .commit();
        fixture.double_roundtrip(client);
        fixture.niri().config.borrow_mut().shrink_expand.enabled = enabled;
        let (window, pointer) = spatial_test_pointer(&mut fixture, ResizeEdge::RIGHT);
        let before = fixture.niri().layout.focus().unwrap().requested_size();
        // No frame between final motion and release.
        pointer.motion(
            fixture.niri_state(),
            None,
            &MotionEvent {
                location: (400. + delta, 300.).into(),
                serial: Serial::from(406),
                time: InputTime::now(),
            },
        );
        pointer.button(
            fixture.niri_state(),
            &ButtonEvent {
                serial: Serial::from(407),
                time: InputTime::now(),
                button: 0x110,
                state: ButtonState::Released,
            },
        );
        assert_eq!(fixture.niri().layout.is_spine(&window), fold);
        if !fold {
            assert_eq!(
                fixture.niri().layout.focus().unwrap().requested_size(),
                before
            );
        }
        assert!(!pointer.is_grabbed());
    }
}

#[test]
fn keyboard_minimum_boundary_repeats_keep_axis_and_grow_restores_native_resize() {
    use crate::shrink_expand::{Kind, Phase};
    use niri_config::Action;
    use niri_ipc::SizeChange;
    for floating in [false, true] {
        for axis in [ShrinkAxis::Horizontal, ShrinkAxis::Vertical] {
            let (mut fixture, client, surfaces) = timed_shrink_fixture(2, floating, 60000);
            fixture
                .client(client)
                .window(surfaces.last().unwrap())
                .xdg_toplevel
                .set_min_size(150, 120);
            fixture
                .client(client)
                .window(surfaces.last().unwrap())
                .commit();
            fixture.double_roundtrip(client);
            let window = fixture.niri().layout.focus().unwrap().window.clone();
            let focus = fixture.niri().keyboard_focus.surface().cloned();
            let action = |change| match axis {
                ShrinkAxis::Horizontal => Action::SetColumnWidth(change),
                ShrinkAxis::Vertical => Action::SetWindowHeight(change),
            };
            fixture
                .niri_state()
                .do_action(action(SizeChange::AdjustFixed(-10000)), false);
            fixture.double_roundtrip(client);
            assert!(fixture.niri().shrink_expand.preview().is_none());
            assert!(!fixture.niri().layout.is_spine(&window));
            fixture
                .niri_state()
                .do_action(action(SizeChange::AdjustFixed(-10)), false);
            fixture.double_roundtrip(client);
            let first = fixture
                .niri()
                .shrink_expand
                .preview()
                .unwrap()
                .record
                .clone();
            assert_eq!(first.kind, Kind::ShrinkTimed(axis));
            assert_eq!(fixture.niri().keyboard_focus.surface().cloned(), focus);
            fixture
                .niri_state()
                .do_action(action(SizeChange::AdjustFixed(0)), false);
            fixture.double_roundtrip(client);
            assert_eq!(
                fixture
                    .niri()
                    .shrink_expand
                    .preview()
                    .unwrap()
                    .record
                    .revision,
                first.revision
            );
            fixture
                .niri_state()
                .do_action(action(SizeChange::AdjustProportion(-1.)), false);
            fixture.double_roundtrip(client);
            let repeated = fixture
                .niri()
                .shrink_expand
                .preview()
                .unwrap()
                .record
                .clone();
            assert_eq!(repeated.id, first.id);
            assert!(repeated.revision > first.revision);
            assert_eq!(repeated.kind, first.kind);
            fixture
                .niri_state()
                .do_action(action(SizeChange::SetFixed(220)), false);
            fixture.double_roundtrip(client);
            assert!(fixture.niri().shrink_expand.preview().is_none());
            assert!(fixture.niri().shrink_preview_timer.is_none());
            assert!(!fixture.niri().layout.is_spine(&window));
            let mapped = fixture.niri().layout.focus().unwrap();
            let requested = mapped.requested_size().unwrap();
            assert_eq!(
                match axis {
                    ShrinkAxis::Horizontal => requested.w,
                    ShrinkAxis::Vertical => requested.h,
                },
                220
            );
            assert!(fixture
                .niri()
                .shrink_expand
                .records()
                .any(|r| r.id == first.id && r.phase == Phase::Cancelled));
            assert!(fixture
                .niri_state()
                .confirm_shrink_preview(repeated.ticket())
                .is_none());
        }
    }
}

#[test]
fn keyboard_boundary_by_id_requires_actual_focus_and_free_axis_keeps_deadline() {
    use niri_config::Action;
    use niri_ipc::SizeChange;
    let (mut fixture, client, _) = timed_shrink_fixture(2, true, 60000);
    let target = fixture.niri().layout.focus().unwrap().id().get();
    let other = fixture
        .niri()
        .layout
        .windows()
        .find(|(_, w)| w.id().get() != target)
        .unwrap()
        .1
        .id()
        .get();
    fixture.niri_state().do_action(
        Action::SetWindowWidthById {
            id: other,
            change: SizeChange::SetFixed(1),
        },
        false,
    );
    fixture.niri_state().do_action(
        Action::SetWindowWidthById {
            id: other,
            change: SizeChange::AdjustFixed(-1),
        },
        false,
    );
    fixture.double_roundtrip(client);
    assert!(fixture.niri().shrink_expand.preview().is_none());
    fixture.niri_state().do_action(Action::ShrinkWindow, false);
    fixture.double_roundtrip(client);
    let before = fixture.niri().shrink_expand.preview().unwrap().clone();
    assert_eq!(
        before.record.kind,
        crate::shrink_expand::Kind::ShrinkTimed(ShrinkAxis::Horizontal)
    );
    let pose = fixture
        .niri()
        .layout
        .active_workspace()
        .unwrap()
        .tiles()
        .find(|tile| tile.window().id().get() == target)
        .unwrap()
        .shrink_preview_pose()
        .unwrap();
    fixture
        .niri_state()
        .do_action(Action::SetWindowHeight(SizeChange::AdjustFixed(50)), false);
    fixture.double_roundtrip(client);
    let after = fixture.niri().shrink_expand.preview().unwrap();
    assert_eq!(after.record, before.record);
    assert_eq!(after.deadline, before.deadline);
    let changed_pose = fixture
        .niri()
        .layout
        .active_workspace()
        .unwrap()
        .tiles()
        .find(|tile| tile.window().id().get() == target)
        .unwrap()
        .shrink_preview_pose()
        .unwrap();
    assert!(changed_pose.h > pose.h);
    assert_eq!(changed_pose.w, pose.w);
    fixture.niri_state().do_action(
        Action::SetWindowWidthById {
            id: target,
            change: SizeChange::AdjustFixed(10),
        },
        false,
    );
    fixture.double_roundtrip(client);
    assert!(fixture.niri().shrink_expand.preview().is_none());
}

#[test]
fn shrink_ipc_disabled_and_pointer_grab_rejections_preserve_native_ownership() {
    use crate::input::click_grab::ClickGrab;
    use niri_ipc::{Action, Request, Response, ShrinkExpandOutcome};
    use smithay::input::pointer::{Focus, GrabStartData};
    use smithay::utils::Serial;
    let (mut fixture, _, _) = timed_shrink_fixture(1, false, 60000);
    let target = fixture.niri().layout.focus().unwrap().window.clone();
    let mut ipc = ShrinkIpcClient::new(&mut fixture);
    fixture.niri().config.borrow_mut().shrink_expand.enabled = false;
    assert!(ipc
        .request(&mut fixture, Request::Action(Action::ShrinkWindow {}))
        .unwrap_err()
        .contains("disabled"));
    assert!(!fixture.niri().layout.is_spine(&target));
    fixture.niri().config.borrow_mut().shrink_expand.enabled = true;
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
        Action::ShrinkWindow {},
        Action::ExpandWindow { id: u64::MAX },
        Action::ExpandAllWindows {},
    ] {
        assert!(ipc
            .request(&mut fixture, Request::Action(action))
            .unwrap_err()
            .contains("blocked"));
        assert!(pointer.is_grabbed());
        assert_eq!(fixture.niri().layout.focus().unwrap().window, target);
        assert!(fixture.niri().shrink_expand.preview().is_none());
    }
    pointer.unset_grab(
        fixture.niri_state(),
        Serial::from(502),
        smithay::backend::input::InputTime::now(),
    );
    ipc.request(&mut fixture, Request::Action(Action::ShrinkWindow {}))
        .unwrap();
    let Response::ShrinkExpandAction(cancelled) = ipc
        .request(
            &mut fixture,
            Request::Action(Action::ExpandWindow { id: u64::MAX }),
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(cancelled.outcome, ShrinkExpandOutcome::Cancelled);
    assert!(!fixture.niri().layout.is_spine(&target));
}

impl ShrinkIpcClient {
    fn new(fixture: &mut Fixture) -> Self {
        let (client, server) = std::os::unix::net::UnixStream::pair().unwrap();
        client.set_nonblocking(true).unwrap();
        crate::ipc::server::attach_test_client(fixture.niri_state(), server);
        Self(std::io::BufReader::new(client))
    }

    fn write(&mut self, request: &niri_ipc::Request) {
        use std::io::Write as _;
        writeln!(
            self.0.get_mut(),
            "{}",
            serde_json::to_string(request).unwrap()
        )
        .unwrap();
    }

    fn read<T: serde::de::DeserializeOwned>(&mut self, fixture: &mut Fixture) -> T {
        use std::io::BufRead as _;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut line = String::new();
        loop {
            fixture.dispatch();
            match self.0.read_line(&mut line) {
                Ok(0) => panic!("IPC closed before response"),
                Ok(_) if line.ends_with('\n') => return serde_json::from_str(&line).unwrap(),
                Ok(_) => (),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(e) => panic!("IPC read: {e}"),
            }
            assert!(
                std::time::Instant::now() < deadline,
                "IPC response timeout: {line}"
            );
            std::thread::yield_now();
        }
    }

    fn request(&mut self, fixture: &mut Fixture, request: niri_ipc::Request) -> niri_ipc::Reply {
        self.write(&request);
        self.read(fixture)
    }
}

#[test]
fn shrink_ipc_dispatch_receipts_snapshot_and_inventory_follow_real_submission() {
    use niri_ipc::{
        Action, Request, Response, ShrinkExpandOutcome as Outcome, ShrinkExpandPhase as Phase,
    };
    let (mut fixture, client, _) = timed_shrink_fixture(2, false, 60000);
    let target = fixture.niri().layout.focus().unwrap().id().get();
    let mut ipc = ShrinkIpcClient::new(&mut fixture);
    assert!(matches!(
        ipc.request(&mut fixture, Request::Action(Action::FocusColumnLeft {}))
            .unwrap(),
        Response::Handled
    ));
    // Restore the original ordinary focus through the public action path.
    assert!(matches!(
        ipc.request(
            &mut fixture,
            Request::Action(Action::FocusWindow { id: target })
        )
        .unwrap(),
        Response::Handled
    ));
    let Response::ShrinkExpandAction(first) = ipc
        .request(&mut fixture, Request::Action(Action::ShrinkWindow {}))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(first.outcome, Outcome::Previewing);
    assert_eq!(first.targets, vec![target]);
    assert!(!first.waiting_for_clients);
    let Response::Windows(windows) = ipc.request(&mut fixture, Request::Windows).unwrap() else {
        panic!()
    };
    assert_eq!(
        windows
            .iter()
            .find(|w| w.id == target)
            .unwrap()
            .shrink_state,
        niri_ipc::ShrinkState::Normal
    );
    let Response::ShrinkExpandAction(repeated) = ipc
        .request(&mut fixture, Request::Action(Action::ShrinkWindow {}))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(repeated.operation_id, first.operation_id);
    let Response::ShrinkExpandAction(submitted) = ipc
        .request(
            &mut fixture,
            Request::Action(Action::ConfirmShrinkExpand {}),
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(submitted.outcome, Outcome::Submitted);
    assert!(!submitted.waiting_for_clients);
    let Response::ShrinkExpandState(state) = ipc
        .request(&mut fixture, Request::ShrinkExpandState)
        .unwrap()
    else {
        panic!()
    };
    assert!(state
        .operations
        .iter()
        .any(|op| Some(op.operation_id) == first.operation_id && op.phase == Phase::Settled));
    let Response::Windows(windows) = ipc.request(&mut fixture, Request::Windows).unwrap() else {
        panic!()
    };
    let book = windows.iter().find(|w| w.id == target).unwrap();
    assert_eq!(book.shrink_state, niri_ipc::ShrinkState::Spine);
    assert_eq!(book.shrink_axis, Some(niri_ipc::ShrinkAxis::Vertical));
    assert_eq!(windows.len(), 2);
    fixture.niri_state().reload_config(Ok(expand_test_config()));
    fixture.double_roundtrip(client);
    let Response::ShrinkExpandAction(expand) = ipc
        .request(
            &mut fixture,
            Request::Action(Action::ExpandWindow { id: target }),
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(expand.outcome, Outcome::Submitted);
    assert!(expand.waiting_for_clients);
    assert_ne!(expand.operation_id, first.operation_id);
    let Response::ShrinkExpandState(state) = ipc
        .request(&mut fixture, Request::ShrinkExpandState)
        .unwrap()
    else {
        panic!()
    };
    assert!(state
        .operations
        .iter()
        .any(|op| Some(op.operation_id) == expand.operation_id && op.phase == Phase::Submitted));
    let Response::Windows(windows) = ipc.request(&mut fixture, Request::Windows).unwrap() else {
        panic!()
    };
    let normal = windows.iter().find(|w| w.id == target).unwrap();
    assert_eq!(normal.shrink_state, niri_ipc::ShrinkState::Normal);
    assert_eq!(normal.shrink_axis, None);
    assert!(ipc
        .request(
            &mut fixture,
            Request::Action(Action::ExpandWindow { id: u64::MAX })
        )
        .is_err());
}

#[test]
fn shrink_ipc_event_prefix_covers_finished_operation_and_only_revision_changes_emit() {
    use niri_ipc::{Action, Event, Request, Response, ShrinkExpandPhase as Phase};
    let (mut fixture, client, _) = timed_shrink_fixture(1, false, 60000);
    let mut control = ShrinkIpcClient::new(&mut fixture);
    let Response::ShrinkExpandAction(first) = control
        .request(&mut fixture, Request::Action(Action::ShrinkWindow {}))
        .unwrap()
    else {
        panic!()
    };
    let Response::ShrinkExpandAction(cancelled) = control
        .request(&mut fixture, Request::Action(Action::CancelShrinkExpand {}))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(cancelled.operation_id, first.operation_id);
    let mut events = ShrinkIpcClient::new(&mut fixture);
    assert!(matches!(
        events.request(&mut fixture, Request::EventStream).unwrap(),
        Response::Handled
    ));
    loop {
        if let Event::ShrinkExpandChanged { state } = events.read(&mut fixture) {
            assert!(state
                .operations
                .iter()
                .any(|op| Some(op.operation_id) == first.operation_id
                    && op.phase == Phase::Cancelled));
            break;
        }
    }
    fixture.double_roundtrip(client);
    // Static refreshes must not produce operation events or per-frame countdown updates.
    for _ in 0..16 {
        fixture.double_roundtrip(client);
    }
    {
        use std::io::BufRead as _;
        loop {
            let mut line = String::new();
            match events.0.read_line(&mut line) {
                Ok(0) => panic!("event stream disconnected"),
                Ok(_) => assert!(!matches!(
                    serde_json::from_str::<Event>(&line).unwrap(),
                    Event::ShrinkExpandChanged { .. }
                )),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(line.is_empty());
                    break;
                }
                Err(e) => panic!("event stream: {e}"),
            }
        }
    }
    let second = control
        .request(&mut fixture, Request::Action(Action::ShrinkWindow {}))
        .unwrap();
    let Response::ShrinkExpandAction(second) = second else {
        panic!()
    };
    loop {
        if let Event::ShrinkExpandChanged { state } = events.read(&mut fixture) {
            assert!(state
                .operations
                .iter()
                .any(|op| Some(op.operation_id) == second.operation_id
                    && op.phase == Phase::Previewing));
            break;
        }
    }
    control
        .request(
            &mut fixture,
            Request::Action(Action::ConfirmShrinkExpand {}),
        )
        .unwrap();
    let mut phases = Vec::new();
    while phases.last() != Some(&Phase::Settled) {
        if let Event::ShrinkExpandChanged { state } = events.read(&mut fixture) {
            let op = state
                .operations
                .iter()
                .find(|op| Some(op.operation_id) == second.operation_id)
                .unwrap();
            phases.push(op.phase);
        }
    }
    assert_eq!(phases, [Phase::Submitted, Phase::Settled]);
}

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
fn early_shrink_confirmation_settles_and_hands_off_focus_before_visual_completion() {
    use crate::shrink_expand::{FinishReason, Phase};
    for floating in [false, true] {
        for count in [1, 2] {
            let (mut fixture, client, _) = timed_shrink_fixture(count, floating, 60000);
            let mut config = niri_config::Config::default();
            config.shrink_expand.enabled = true;
            config.shrink_expand.shrink_buffer_ms = 60000;
            config.animations.window_movement.0 = niri_config::Animation {
                off: false,
                kind: niri_config::animations::Kind::Easing(
                    niri_config::animations::EasingParams {
                        duration_ms: 2000,
                        curve: niri_config::animations::Curve::Linear,
                    },
                ),
            };
            fixture.niri().layout.update_config(&config);
            *fixture.niri().config.borrow_mut() = config;
            let target = fixture.niri().layout.focus().unwrap().window.clone();
            let record = fixture.niri_state().begin_shrink_preview(None).unwrap();
            fixture
                .niri_state()
                .confirm_shrink_preview(record.ticket())
                .unwrap();
            assert!(fixture.niri().layout.is_spine(&target));
            let ongoing = fixture
                .niri()
                .layout
                .workspaces()
                .flat_map(|(_, _, ws)| ws.tiles())
                .find(|tile| crate::layout::LayoutElement::id(tile.window()) == &target)
                .unwrap()
                .are_transitions_ongoing();
            assert!(ongoing);
            assert!(fixture.niri().shrink_preview_timer.is_none());
            assert!(fixture.niri().shrink_expand.preview().is_none());
            assert!(fixture
                .niri()
                .shrink_expand
                .records()
                .any(|current| current.id == record.id
                    && current.phase == Phase::Settled
                    && current.reason == Some(FinishReason::Ready)));
            fixture.double_roundtrip(client);
            if count == 1 {
                assert!(fixture.niri().keyboard_focus.surface().is_none());
            } else {
                let focus = fixture.niri().layout.focus().unwrap().window.clone();
                assert_ne!(focus, target);
                assert!(!fixture.niri().layout.is_spine(&focus));
                assert!(fixture.niri().keyboard_focus.surface().is_some());
            }
            fixture.niri_complete_animations();
            assert!(fixture
                .niri()
                .layout
                .workspaces()
                .flat_map(|(_, _, ws)| ws.tiles())
                .filter(|tile| crate::layout::LayoutElement::id(tile.window()) == &target)
                .all(|tile| !tile.are_transitions_ongoing()));
            assert_eq!(fixture.niri().layout.windows().count(), count);
        }
    }
}

#[test]
fn shrink_and_expand_preserve_the_actual_input_method_keyboard_grab() {
    use crate::shrink_expand::{FinishReason, Phase};
    use smithay::utils::Serial;
    use smithay::wayland::input_method::InputMethodKeyboardGrab;
    for floating in [false, true] {
        let (mut fixture, client, surfaces) = timed_shrink_fixture(2, floating, 60000);
        let target = fixture.niri().layout.focus().unwrap().window.clone();
        let target_id = fixture.niri().layout.focus().unwrap().id().get();
        let surface = fixture.niri().keyboard_focus.surface().unwrap().clone();
        let keyboard = fixture.niri().seat.get_keyboard().unwrap();
        // The actual Smithay type, without an external text-input client.
        keyboard.set_grab(
            fixture.niri_state(),
            InputMethodKeyboardGrab::default(),
            Serial::from(701),
        );
        let first = fixture
            .niri_state()
            .begin_shrink_preview(Some(ShrinkAxis::Horizontal))
            .unwrap();
        fixture.double_roundtrip(client);
        assert_eq!(fixture.niri().keyboard_focus.surface(), Some(&surface));
        assert!(keyboard
            .with_grab(|_, grab| grab.is::<InputMethodKeyboardGrab>())
            .unwrap());
        let repeated = fixture.niri_state().begin_shrink_preview(None).unwrap();
        assert_eq!(first.id, repeated.id);
        fixture.niri_state().cancel_shrink_expand_preview().unwrap();
        fixture.double_roundtrip(client);
        assert!(!fixture.niri().layout.is_spine(&target));
        assert_eq!(fixture.niri().keyboard_focus.surface(), Some(&surface));
        assert!(keyboard
            .with_grab(|_, grab| grab.is::<InputMethodKeyboardGrab>())
            .unwrap());
        let record = fixture
            .niri_state()
            .begin_shrink_preview(Some(ShrinkAxis::Horizontal))
            .unwrap();
        fixture
            .niri_state()
            .confirm_shrink_preview(record.ticket())
            .unwrap();
        fixture.double_roundtrip(client);
        assert!(fixture.niri().layout.is_spine(&target));
        assert_ne!(fixture.niri().keyboard_focus.surface(), Some(&surface));
        assert!(keyboard
            .with_grab(|_, grab| grab.is::<InputMethodKeyboardGrab>())
            .unwrap());
        fixture.niri_state().reload_config(Ok(expand_test_config()));
        let expand = fixture
            .niri_state()
            .expand_spine_window(target_id)
            .unwrap()
            .unwrap();
        fixture.double_roundtrip(client);
        assert!(keyboard
            .with_grab(|_, grab| grab.is::<InputMethodKeyboardGrab>())
            .unwrap());
        let window = fixture.client(client).window(surfaces.last().unwrap());
        let size = window.configures_received.last().unwrap().1.size;
        window.set_size(size.0 as u16, size.1 as u16);
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
        assert_eq!(fixture.niri().keyboard_focus.surface(), Some(&surface));
        assert!(keyboard
            .with_grab(|_, grab| grab.is::<InputMethodKeyboardGrab>())
            .unwrap());
        assert!(fixture
            .niri()
            .shrink_expand
            .records()
            .any(|current| current.id == expand.id
                && current.phase == Phase::Settled
                && current.reason == Some(FinishReason::Ready)));
        assert_eq!(fixture.niri().layout.windows().count(), 2);
    }
}

#[test]
fn actual_session_lock_cancels_shrink_preview_and_invalidates_its_old_ticket() {
    use crate::shrink_expand::{Phase, Rejected};
    for floating in [false, true] {
        let (mut fixture, client, _) = timed_shrink_fixture(2, floating, 60000);
        let target = fixture.niri().layout.focus().unwrap().window.clone();
        let record = fixture.niri_state().begin_shrink_preview(None).unwrap();
        let lock = super::sink::lock_fixture(&mut fixture, client);
        assert!(fixture.niri().shrink_expand.preview().is_none());
        assert!(fixture.niri().shrink_preview_timer.is_none());
        assert!(!fixture.niri().layout.is_spine(&target));
        assert!(fixture
            .niri()
            .layout
            .workspaces()
            .flat_map(|(_, _, ws)| ws.tiles())
            .all(|tile| tile.shrink_preview_pose().is_none()));
        assert!(fixture
            .niri_state()
            .confirm_shrink_preview(record.ticket())
            .is_none());
        assert_eq!(
            fixture.niri_state().begin_shrink_preview(None),
            Err(Rejected::Blocked)
        );
        assert!(fixture
            .niri()
            .shrink_expand
            .records()
            .any(|current| current.id == record.id && current.phase == Phase::Cancelled));
        lock.unlock_and_destroy();
        fixture.double_roundtrip(client);
        assert!(!fixture.niri().is_locked());
        assert!(fixture
            .niri_state()
            .confirm_shrink_preview(record.ticket())
            .is_none());
        assert!(!fixture.niri().layout.is_spine(&target));
        assert_eq!(fixture.niri().layout.windows().count(), 2);
    }
}

#[test]
fn actual_session_lock_cancels_deferred_expand_focus_without_rearming_the_deadline() {
    use crate::shrink_expand::{FinishReason, Phase};
    for floating in [false, true] {
        for reply_under_lock in [false, true] {
            let (mut fixture, client, surfaces) = timed_shrink_fixture(2, floating, 60000);
            let target = fixture.niri().layout.focus().unwrap().window.clone();
            let target_id = fixture.niri().layout.focus().unwrap().id().get();
            let target_surface = surfaces.last().unwrap().clone();
            let target_server_surface = fixture.niri().keyboard_focus.surface().unwrap().clone();
            let shrink = fixture
                .niri_state()
                .begin_shrink_preview(Some(ShrinkAxis::Horizontal))
                .unwrap();
            fixture
                .niri_state()
                .confirm_shrink_preview(shrink.ticket())
                .unwrap();
            fixture.double_roundtrip(client);
            let receiver = fixture.niri().layout.focus().unwrap().window.clone();
            fixture.niri_state().reload_config(Ok(expand_test_config()));
            let record = fixture
                .niri_state()
                .expand_spine_window(target_id)
                .unwrap()
                .unwrap();
            fixture.double_roundtrip(client);
            assert!(fixture.niri().shrink_expand_focus.is_some());
            let deadline = fixture
                .niri()
                .layout
                .windows()
                .find(|(_, win)| win.window == target)
                .unwrap()
                .1
                .normal_restore_deadline()
                .unwrap();
            let window = fixture.client(client).window(&target_surface);
            let size = window.configures_received.last().unwrap().1.size;
            window.ack_last();
            fixture.double_roundtrip(client);
            let lock = super::sink::lock_fixture(&mut fixture, client);
            assert!(fixture.niri().shrink_expand_focus.is_none());
            assert_eq!(
                fixture
                    .niri()
                    .layout
                    .windows()
                    .find(|(_, win)| win.window == target)
                    .unwrap()
                    .1
                    .normal_restore_deadline(),
                Some(deadline)
            );
            if !reply_under_lock {
                lock.unlock_and_destroy();
                fixture.double_roundtrip(client);
                fixture.niri_state().focus_window(&receiver);
                fixture.double_roundtrip(client);
            }
            let window = fixture.client(client).window(&target_surface);
            window.set_size(size.0 as u16, size.1 as u16);
            window.commit();
            fixture.double_roundtrip(client);
            assert!(fixture
                .niri()
                .shrink_expand
                .records()
                .any(|current| current.id == record.id
                    && current.phase == Phase::Settled
                    && current.reason == Some(FinishReason::Ready)));
            assert_ne!(
                fixture.niri().keyboard_focus.surface(),
                Some(&target_server_surface)
            );
            if reply_under_lock {
                lock.unlock_and_destroy();
                fixture.double_roundtrip(client);
            }
            assert!(!fixture.niri().is_locked());
            assert!(fixture.niri().shrink_expand_focus.is_none());
            assert_ne!(
                fixture.niri().keyboard_focus.surface(),
                Some(&target_server_surface)
            );
            assert!(fixture
                .niri_state()
                .confirm_shrink_preview(shrink.ticket())
                .is_none());
            assert_eq!(fixture.niri().layout.windows().count(), 2);
        }
    }
}

struct SelectionFixture {
    fixture: Fixture,
    client: super::client::ClientId,
    surfaces: Vec<wayland_client::protocol::wl_surface::WlSurface>,
    windows: Vec<(u64, smithay::desktop::Window)>,
}

fn selection_fixture(all: bool, floating: bool) -> SelectionFixture {
    let (mut fixture, client, surfaces) = timed_shrink_fixture(4, floating, 60000);
    fixture
        .niri()
        .config
        .borrow_mut()
        .shrink_expand
        .expand_buffer_ms = 60000;
    let mut windows = fixture
        .niri()
        .layout
        .windows()
        .map(|(_, window)| (window.id().get(), window.window.clone()))
        .collect::<Vec<_>>();
    windows.sort_by_key(|(id, _)| *id);
    for (index, (_, window)) in windows.iter().enumerate() {
        if all || index > 0 {
            assert!(fixture
                .niri()
                .layout
                .commit_shrink(window, ShrinkAxis::Horizontal, 32.));
        }
        if floating {
            use niri_ipc::PositionChange;
            fixture.niri().layout.move_floating_window(
                Some(window),
                PositionChange::SetFixed(100. + index as f64 * 100.),
                PositionChange::SetFixed(150.),
                false,
            );
        }
    }
    if !all {
        fixture.niri_state().focus_window(&windows[0].1);
    }
    fixture.double_roundtrip(client);
    fixture.niri_complete_animations();
    SelectionFixture {
        fixture,
        client,
        surfaces,
        windows,
    }
}

#[test]
fn expand_selection_cycles_without_configuring_or_replacing_the_original_focus() {
    use crate::shrink_expand::{Kind, Phase};
    for all in [false, true] {
        for floating in [false, true] {
            let SelectionFixture {
                mut fixture,
                client,
                surfaces,
                windows,
            } = selection_fixture(all, floating);
            if floating && !all {
                assert_eq!(
                    fixture.niri_state().begin_expand_selection(),
                    Err(crate::shrink_expand::Rejected::NoTarget)
                );
                continue;
            }
            let old_focus = fixture.niri().keyboard_focus.surface().cloned();
            let before = surfaces
                .iter()
                .map(|surface| {
                    fixture
                        .client(client)
                        .window(surface)
                        .configures_received
                        .len()
                })
                .collect::<Vec<_>>();
            let candidate_ids = windows
                .iter()
                .skip(if all { 0 } else { 1 })
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            let first = fixture.niri_state().begin_expand_selection().unwrap();
            assert_eq!(first.kind, Kind::ExpandSelection);
            assert_eq!(first.phase, Phase::Previewing);
            assert_eq!(first.targets, vec![candidate_ids[0]]);
            for expected in candidate_ids
                .iter()
                .skip(1)
                .chain(std::iter::once(&candidate_ids[0]))
            {
                let repeated = fixture.niri_state().begin_expand_selection().unwrap();
                assert_eq!(repeated.id, first.id);
                assert!(repeated.revision > first.revision);
                assert_eq!(repeated.targets, vec![*expected]);
                assert!(fixture
                    .niri_state()
                    .confirm_shrink_preview(first.ticket())
                    .is_none());
            }
            fixture.double_roundtrip(client);
            assert_eq!(fixture.niri().keyboard_focus.surface().cloned(), old_focus);
            assert_eq!(
                surfaces
                    .iter()
                    .map(|surface| fixture
                        .client(client)
                        .window(surface)
                        .configures_received
                        .len())
                    .collect::<Vec<_>>(),
                before
            );
            assert_eq!(
                fixture
                    .niri_state()
                    .begin_shrink_preview(None)
                    .unwrap()
                    .phase,
                Phase::Cancelled
            );
            assert!(fixture.niri().expand_selection_lease.is_none());
            assert!(fixture.niri().shrink_preview_timer.is_none());
            assert_eq!(fixture.niri().keyboard_focus.surface().cloned(), old_focus);
            for (_, window) in windows.iter().skip(if all { 0 } else { 1 }) {
                assert!(fixture.niri().layout.is_spine(window));
            }
        }
    }
}

#[test]
fn expand_selection_keeps_selected_identity_when_other_candidates_close() {
    let SelectionFixture {
        mut fixture,
        client,
        surfaces,
        windows,
    } = selection_fixture(false, false);
    fixture.niri_state().begin_expand_selection().unwrap();
    let selected = fixture.niri_state().begin_expand_selection().unwrap();
    assert_eq!(selected.targets, vec![windows[2].0]);
    let deadline = fixture.niri().shrink_expand.preview().unwrap().deadline;
    let window = fixture.client(client).window(&surfaces[1]);
    window.xdg_toplevel.destroy();
    window.xdg_surface.destroy();
    window.surface.destroy();
    fixture.double_roundtrip(client);
    let current = fixture.niri().shrink_expand.preview().unwrap();
    assert_eq!(current.record.ticket(), selected.ticket());
    assert_eq!(current.record.targets, selected.targets);
    assert_eq!(current.deadline, deadline);
    assert_eq!(
        fixture
            .niri_state()
            .begin_expand_selection()
            .unwrap()
            .targets,
        vec![windows[3].0]
    );
    assert_eq!(fixture.niri().layout.focus().unwrap().window, windows[0].1);
}

#[test]
fn expand_selection_invalidations_cancel_without_substituting_a_new_target() {
    use crate::shrink_expand::Phase;
    for reason in [
        "selected-close",
        "selected-normal",
        "workspace",
        "lock",
        "disable",
    ] {
        let SelectionFixture {
            mut fixture,
            client,
            surfaces,
            windows,
        } = selection_fixture(false, false);
        let first = fixture.niri_state().begin_expand_selection().unwrap();
        assert_eq!(first.targets, vec![windows[1].0]);
        let mut lock = None;
        match reason {
            "selected-close" => {
                let window = fixture.client(client).window(&surfaces[1]);
                window.xdg_toplevel.destroy();
                window.xdg_surface.destroy();
                window.surface.destroy();
            }
            "selected-normal" => {
                let ws = fixture
                    .niri()
                    .layout
                    .workspaces_mut()
                    .find(|ws| ws.has_window(&windows[1].1))
                    .unwrap();
                ws.restore_normal_spines(std::slice::from_ref(&windows[1].1));
            }
            "workspace" => fixture.niri().layout.switch_workspace_down(),
            "lock" => lock = Some(super::sink::lock_fixture(&mut fixture, client)),
            "disable" => fixture.niri().config.borrow_mut().shrink_expand.enabled = false,
            _ => unreachable!(),
        }
        fixture.double_roundtrip(client);
        assert!(fixture.niri().shrink_expand.preview().is_none(), "{reason}");
        assert!(fixture.niri().expand_selection_lease.is_none(), "{reason}");
        assert!(fixture.niri().shrink_preview_timer.is_none(), "{reason}");
        assert!(fixture
            .niri_state()
            .confirm_shrink_preview(first.ticket())
            .is_none());
        assert!(fixture
            .niri()
            .shrink_expand
            .records()
            .any(|record| record.id == first.id && record.phase == Phase::Cancelled));
        assert!(fixture.niri().layout.is_spine(&windows[2].1));
        assert!(fixture.niri().layout.is_spine(&windows[3].1));
        if let Some(lock) = lock {
            lock.unlock_and_destroy();
            fixture.double_roundtrip(client);
        }
    }
    // A formerly all-spine scope never silently changes into a local search.
    let SelectionFixture {
        mut fixture,
        client,
        surfaces,
        windows,
    } = selection_fixture(true, false);
    let first = fixture.niri_state().begin_expand_selection().unwrap();
    let ws = fixture
        .niri()
        .layout
        .workspaces_mut()
        .find(|ws| ws.has_window(&windows[3].1))
        .unwrap();
    ws.restore_normal_spines(std::slice::from_ref(&windows[3].1));
    let window = fixture.client(client).window(&surfaces[3]);
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    assert!(fixture.niri().shrink_expand.preview().is_none());
    assert!(fixture
        .niri_state()
        .confirm_shrink_preview(first.ticket())
        .is_none());
}

#[test]
fn expand_selection_confirmation_restores_only_the_selected_window_and_operation_id() {
    use crate::shrink_expand::{FinishReason, Kind, Phase};
    for floating in [false, true] {
        let SelectionFixture {
            mut fixture,
            client,
            surfaces,
            windows,
        } = selection_fixture(true, floating);
        let mut config = expand_test_config();
        config.shrink_expand.expand_buffer_ms = 60000;
        fixture.niri_state().reload_config(Ok(config));
        let first = fixture.niri_state().begin_expand_selection().unwrap();
        let selected = fixture.niri_state().begin_expand_selection().unwrap();
        assert_eq!(selected.targets, vec![windows[1].0]);
        assert!(fixture
            .niri_state()
            .confirm_shrink_preview(first.ticket())
            .is_none());
        let submitted = fixture
            .niri_state()
            .confirm_shrink_preview(selected.ticket())
            .unwrap();
        assert_eq!(submitted.id, first.id);
        assert_eq!(submitted.kind, Kind::ExpandSelection);
        assert_eq!(submitted.phase, Phase::Submitted);
        assert!(!fixture.niri().layout.is_spine(&windows[1].1));
        assert!(fixture.niri().expand_selection_lease.is_none());
        assert!(fixture.niri().shrink_preview_timer.is_none());
        for index in [0, 2, 3] {
            assert!(fixture.niri().layout.is_spine(&windows[index].1));
        }
        fixture.double_roundtrip(client);
        assert!(fixture.niri().keyboard_focus.surface().is_none());
        let window = fixture.client(client).window(&surfaces[1]);
        let size = window.configures_received.last().unwrap().1.size;
        window.set_size(size.0 as u16, size.1 as u16);
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
        assert_eq!(fixture.niri().layout.focus().unwrap().window, windows[1].1);
        assert!(fixture
            .niri()
            .shrink_expand
            .records()
            .any(|record| record.id == first.id
                && record.kind == Kind::ExpandSelection
                && record.phase == Phase::Settled
                && record.reason == Some(FinishReason::Ready)));
        assert_eq!(fixture.niri().layout.windows().count(), 4);
    }
}

#[test]
fn expand_selection_zero_timer_confirms_the_current_identity_only() {
    use crate::shrink_expand::{Kind, Phase};
    let SelectionFixture {
        mut fixture,
        client,
        windows,
        ..
    } = selection_fixture(true, false);
    fixture
        .niri()
        .config
        .borrow_mut()
        .shrink_expand
        .expand_buffer_ms = 0;
    let preview = fixture.niri_state().begin_expand_selection().unwrap();
    assert_eq!(preview.targets, vec![windows[0].0]);
    fixture.double_roundtrip(client);
    assert!(!fixture.niri().layout.is_spine(&windows[0].1));
    assert!(windows
        .iter()
        .skip(1)
        .all(|(_, window)| fixture.niri().layout.is_spine(window)));
    assert!(fixture.niri().expand_selection_lease.is_none());
    assert!(fixture.niri().shrink_preview_timer.is_none());
    assert!(fixture
        .niri()
        .shrink_expand
        .records()
        .any(|record| record.id == preview.id
            && record.kind == Kind::ExpandSelection
            && matches!(record.phase, Phase::Submitted | Phase::Settled)));
}

#[test]
fn expand_selection_keeps_normal_client_keys_and_balances_escape_cancellation() {
    use super::client::KeyboardEvent;
    use super::test_input_backend::{TestInputBackend, TestKeyboardKeyEvent};
    use smithay::backend::input::{InputEvent, InputTime, KeyState};
    let SelectionFixture {
        mut fixture,
        client,
        surfaces,
        windows,
    } = selection_fixture(false, false);
    let surface = &surfaces[0];
    let _ = fixture
        .client(client)
        .state
        .recent_keyboard_events(surface)
        .count();
    let preview = fixture.niri_state().begin_expand_selection().unwrap();
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
    send(&mut fixture, "AC01", KeyState::Released);
    assert_eq!(
        fixture
            .client(client)
            .state
            .recent_keyboard_events(surface)
            .filter(|event| matches!(event, KeyboardEvent::Key { .. }))
            .count(),
        2
    );
    assert_eq!(
        fixture
            .niri()
            .shrink_expand
            .preview()
            .unwrap()
            .record
            .ticket(),
        preview.ticket()
    );
    send(&mut fixture, "ESC", KeyState::Pressed);
    send(&mut fixture, "ESC", KeyState::Released);
    assert!(fixture.niri().shrink_expand.preview().is_none());
    assert!(fixture.niri().expand_selection_lease.is_none());
    assert!(fixture.niri().suppressed_keys.is_empty());
    assert_eq!(
        fixture
            .client(client)
            .state
            .recent_keyboard_events(surface)
            .filter(|event| matches!(event, KeyboardEvent::Key { .. }))
            .count(),
        0
    );
    assert_eq!(fixture.niri().layout.focus().unwrap().window, windows[0].1);
    assert!(windows
        .iter()
        .skip(1)
        .all(|(_, window)| fixture.niri().layout.is_spine(window)));
}

fn expand_test_config() -> niri_config::Config {
    niri_config::Config::parse_mem(
        r#"
animations { off; }
shrink-expand { enabled true; }
window-rule {
    default-column-width { fixed 720; }
    default-window-height { fixed 400; }
}
"#,
    )
    .unwrap()
}

#[test]
fn direct_expand_runtime_matching_geometry_and_opposite_input_need_no_extra_operation() {
    use crate::shrink_expand::{FinishReason, Phase, Rejected};
    let (mut fixture, client, _) = timed_shrink_fixture(1, true, 60000);
    let target_id = fixture.niri().layout.focus().unwrap().id().get();
    let surface = fixture.niri().keyboard_focus.surface().cloned();
    for all in [false, true] {
        let preview = fixture.niri_state().begin_shrink_preview(None).unwrap();
        let cancelled = if all {
            fixture.niri_state().expand_all_spines()
        } else {
            fixture.niri_state().expand_spine_window(u64::MAX)
        }
        .unwrap()
        .unwrap();
        assert_eq!(cancelled.id, preview.id);
        assert_eq!(cancelled.phase, Phase::Cancelled);
        fixture.double_roundtrip(client);
        assert_eq!(fixture.niri().keyboard_focus.surface().cloned(), surface);
        assert!(fixture.niri().shrink_preview_timer.is_none());
        assert!(!fixture
            .niri()
            .layout
            .workspaces()
            .flat_map(|(_, _, ws)| ws.tiles())
            .any(|tile| tile.is_spine() || tile.shrink_preview_pose().is_some()));
    }
    assert_eq!(
        fixture.niri_state().expand_spine_window(u64::MAX),
        Err(Rejected::NoTarget)
    );
    let preview = fixture.niri_state().begin_shrink_preview(None).unwrap();
    fixture
        .niri_state()
        .confirm_shrink_preview(preview.ticket())
        .unwrap();
    fixture.double_roundtrip(client);
    fixture.niri().config.borrow_mut().shrink_expand.enabled = false;
    assert_eq!(
        fixture.niri_state().expand_all_spines(),
        Err(Rejected::Disabled)
    );
    assert_eq!(
        fixture.niri_state().expand_spine_window(target_id),
        Err(Rejected::Disabled)
    );
    assert!(fixture
        .niri()
        .layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.tiles())
        .all(|tile| tile.is_spine()));
    fixture.niri().config.borrow_mut().shrink_expand.enabled = true;
    let record = fixture
        .niri_state()
        .expand_spine_window(target_id)
        .unwrap()
        .unwrap();
    fixture.double_roundtrip(client);
    assert!(fixture
        .niri()
        .shrink_expand
        .records()
        .any(|r| r.id == record.id
            && r.phase == Phase::Settled
            && r.reason == Some(FinishReason::Ready)));
    assert!(fixture.niri().shrink_expand_focus.is_none());
    assert_eq!(fixture.niri().keyboard_focus.surface().cloned(), surface);
}

#[test]
fn direct_expand_runtime_waits_for_processed_commit_and_cancels_late_focus() {
    use crate::shrink_expand::{FinishReason, Kind, Phase};
    for floating in [false, true] {
        for navigate in [false, true] {
            let (mut fixture, client, surfaces) = timed_shrink_fixture(2, floating, 60000);
            let target = fixture.niri().layout.focus().unwrap().window.clone();
            let target_id = fixture.niri().layout.focus().unwrap().id().get();
            let shrink = fixture
                .niri_state()
                .begin_shrink_preview(Some(ShrinkAxis::Horizontal))
                .unwrap();
            fixture
                .niri_state()
                .confirm_shrink_preview(shrink.ticket())
                .unwrap();
            fixture.double_roundtrip(client);
            let receiver = fixture.niri().layout.focus().unwrap().window.clone();
            fixture.niri_state().reload_config(Ok(expand_test_config()));
            let record = fixture
                .niri_state()
                .expand_spine_window(target_id)
                .unwrap()
                .unwrap();
            assert_eq!(record.kind, Kind::ExpandDirect);
            assert_eq!(record.phase, Phase::Submitted);
            assert_eq!(record.targets, vec![target_id]);
            assert!(!fixture.niri().layout.is_spine(&target));
            fixture.double_roundtrip(client);
            assert!(fixture.niri().keyboard_focus.surface().is_none());
            assert!(fixture.niri().shrink_expand_focus.is_some());
            assert!(fixture
                .niri()
                .shrink_expand
                .records()
                .any(|r| r.id == record.id && r.phase == Phase::Submitted));
            let window = fixture.client(client).window(surfaces.last().unwrap());
            let size = window.configures_received.last().unwrap().1.size;
            if floating {
                assert_eq!(
                    size,
                    (720, 400),
                    "restore target must precede a reply to the old configure"
                );
            }
            window.ack_last();
            fixture.double_roundtrip(client);
            assert!(fixture
                .niri()
                .shrink_expand
                .records()
                .any(|r| r.id == record.id && r.phase == Phase::Submitted));
            if navigate {
                fixture.niri_state().focus_window(&receiver);
                fixture.double_roundtrip(client);
                assert!(fixture.niri().shrink_expand_focus.is_none());
            }
            let window = fixture.client(client).window(surfaces.last().unwrap());
            window.set_size(size.0 as u16, size.1 as u16);
            window.commit();
            fixture.double_roundtrip(client);
            assert!(
                fixture
                    .niri()
                    .shrink_expand
                    .records()
                    .any(|r| r.id == record.id
                        && r.phase == Phase::Settled
                        && r.reason == Some(FinishReason::Ready)),
                "floating={floating} navigate={navigate}, records={:?}",
                fixture.niri().shrink_expand.records().collect::<Vec<_>>()
            );
            let focused = fixture.niri().layout.focus().unwrap().window.clone();
            assert_eq!(focused, if navigate { receiver } else { target });
            assert!(fixture.niri().keyboard_focus.surface().is_some());
            assert!(fixture.niri().shrink_expand_focus.is_none());
            assert_eq!(fixture.niri().layout.windows().count(), 2);
            assert_eq!(
                fixture.niri_state().expand_spine_window(target_id).unwrap(),
                None
            );
        }
    }
}

#[test]
fn direct_expand_runtime_deadline_and_destroy_have_stable_terminal_results() {
    use crate::shrink_expand::{FinishReason, Phase};
    for floating in [false, true] {
        for close in [false, true] {
            let (mut fixture, client, surfaces) = timed_shrink_fixture(2, floating, 60000);
            let target_id = fixture.niri().layout.focus().unwrap().id().get();
            let shrink = fixture.niri_state().begin_shrink_preview(None).unwrap();
            fixture
                .niri_state()
                .confirm_shrink_preview(shrink.ticket())
                .unwrap();
            fixture.double_roundtrip(client);
            let receiver = fixture.niri().layout.focus().unwrap().window.clone();
            fixture.niri_state().reload_config(Ok(expand_test_config()));
            let record = fixture
                .niri_state()
                .expand_spine_window(target_id)
                .unwrap()
                .unwrap();
            fixture.double_roundtrip(client);
            let deadline = fixture
                .niri()
                .layout
                .windows()
                .find(|(_, w)| w.id().get() == target_id)
                .unwrap()
                .1
                .normal_restore_deadline()
                .unwrap();
            fixture.niri_state().focus_window(&receiver);
            fixture.double_roundtrip(client);
            let window = fixture.client(client).window(surfaces.last().unwrap());
            if close {
                window.xdg_toplevel.destroy();
                window.xdg_surface.destroy();
                window.surface.destroy();
            } else {
                window.ack_last();
                fixture.double_roundtrip(client);
                std::thread::sleep(
                    deadline.saturating_duration_since(std::time::Instant::now())
                        + std::time::Duration::from_millis(10),
                );
            }
            fixture.double_roundtrip(client);
            let reason = if close {
                FinishReason::TargetsRemoved
            } else {
                FinishReason::Deadline
            };
            assert!(fixture
                .niri()
                .shrink_expand
                .records()
                .any(|r| r.id == record.id
                    && r.phase == Phase::Settled
                    && r.reason == Some(reason)));
            if !close {
                let window = fixture.client(client).window(surfaces.last().unwrap());
                let size = window.configures_received.last().unwrap().1.size;
                window.set_size(size.0 as u16, size.1 as u16);
                window.commit();
                fixture.double_roundtrip(client);
                assert!(fixture
                    .niri()
                    .shrink_expand
                    .records()
                    .any(|r| r.id == record.id && r.reason == Some(reason)));
            }
            assert_eq!(fixture.niri().layout.focus().unwrap().window, receiver);
            assert!(fixture.niri().shrink_expand_focus.is_none());
        }
    }
}

#[test]
fn expand_all_runtime_restores_hidden_tabs_and_floating_without_focus_jump() {
    use crate::shrink_expand::{FinishReason, Kind, Phase};
    for all_books in [false, true] {
        let (mut fixture, client, _) = timed_shrink_fixture(4, false, 60000);
        let mut windows = fixture
            .niri()
            .layout
            .windows()
            .map(|(_, w)| (w.id().get(), w.window.clone()))
            .collect::<Vec<_>>();
        windows.sort_by_key(|(id, _)| *id);
        fixture
            .niri()
            .layout
            .set_window_floating(Some(&windows[0].1), true);
        fixture
            .niri()
            .layout
            .consume_or_expel_window_left(Some(&windows[2].1));
        fixture
            .niri()
            .layout
            .consume_or_expel_window_left(Some(&windows[3].1));
        fixture.niri().layout.activate_window(&windows[3].1);
        fixture
            .niri()
            .layout
            .set_column_display(niri_ipc::ColumnDisplay::Tabbed);
        fixture
            .niri()
            .layout
            .set_column_width(niri_ipc::SizeChange::SetFixed(900));
        let ws = fixture.niri().layout.active_workspace().unwrap();
        assert_eq!(ws.scrolling().columns().count(), 1);
        assert_eq!(
            ws.scrolling()
                .tiles_with_render_positions()
                .filter(|(_, _, visible)| !*visible)
                .count(),
            2
        );
        assert!(ws.floating().has_window(&windows[0].1));
        let targets = &windows[..if all_books { 4 } else { 3 }];
        for (i, (_, id)) in targets.iter().enumerate() {
            let ws = fixture
                .niri()
                .layout
                .workspaces_mut()
                .find(|ws| ws.has_window(id))
                .unwrap();
            let tile = ws
                .tiles_mut()
                .find(|tile| &tile.window().window == id)
                .unwrap();
            assert!(tile.set_spine(
                if i % 2 == 0 {
                    ShrinkAxis::Horizontal
                } else {
                    ShrinkAxis::Vertical
                },
                32.
            ));
            ws.update_window(id, None);
        }
        fixture.niri_state().reload_config(Ok(expand_test_config()));
        fixture.double_roundtrip(client);
        let focus = fixture.niri().keyboard_focus.surface().cloned();
        assert_eq!(focus.is_none(), all_books);
        let record = fixture.niri_state().expand_all_spines().unwrap().unwrap();
        assert_eq!(record.kind, Kind::ExpandAll);
        let mut actual = record.targets.clone();
        actual.sort();
        assert_eq!(
            actual,
            targets.iter().map(|(id, _)| *id).collect::<Vec<_>>()
        );
        fixture.double_roundtrip(client);
        assert_eq!(fixture.niri().keyboard_focus.surface().cloned(), focus);
        assert!(fixture.niri().shrink_expand_focus.is_none());
        assert!(fixture
            .niri()
            .layout
            .workspaces()
            .flat_map(|(_, _, ws)| ws.tiles())
            .all(|tile| !tile.is_spine()));
        let deadlines = fixture
            .niri()
            .layout
            .windows()
            .filter(|(_, w)| actual.contains(&w.id().get()))
            .filter_map(|(_, w)| w.normal_restore_deadline())
            .collect::<Vec<_>>();
        assert_eq!(deadlines.len(), targets.len());
        assert!(deadlines.iter().all(|at| *at == deadlines[0]));
        let surfaces = fixture
            .client(client)
            .state
            .windows
            .iter()
            .map(|w| w.surface.clone())
            .collect::<Vec<_>>();
        for surface in &surfaces {
            let window = fixture.client(client).window(surface);
            let size = window.configures_received.last().unwrap().1.size;
            window.set_size(size.0 as u16, size.1 as u16);
            window.ack_last_and_commit();
        }
        fixture.double_roundtrip(client);
        assert!(fixture
            .niri()
            .shrink_expand
            .records()
            .any(|r| r.id == record.id
                && r.phase == Phase::Settled
                && r.reason == Some(FinishReason::Ready)));
        assert_eq!(fixture.niri().keyboard_focus.surface().cloned(), focus);
        assert_eq!(fixture.niri_state().expand_all_spines().unwrap(), None);
        assert!(fixture
            .niri()
            .layout
            .active_workspace()
            .unwrap()
            .floating()
            .has_window(&windows[0].1));
    }
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
            assert!(fixture
                .niri()
                .layout
                .workspaces()
                .flat_map(|(_, _, ws)| ws.tiles())
                .any(
                    |tile| crate::layout::LayoutElement::id(tile.window()) == &window
                        && tile.shrink_preview_pose().is_some()
                ));
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
            assert!(fixture
                .niri()
                .layout
                .workspaces()
                .flat_map(|(_, _, ws)| ws.tiles())
                .all(|tile| tile.shrink_preview_pose().is_none()));
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
        for tile in fixture
            .niri()
            .layout
            .workspaces()
            .flat_map(|(_, _, ws)| ws.tiles())
        {
            assert!(tile.shrink_preview_pose().is_none(), "{change}");
        }
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
