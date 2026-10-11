use smithay::reexports::wayland_server::Resource as _;
use wayland_client::Proxy as _;

use super::Fixture;
use crate::layout::LayoutElement as _;

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
