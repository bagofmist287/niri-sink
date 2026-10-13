use std::collections::HashMap;
use std::time::Duration;

use niri::animation::Clock;
use niri::layout::shrink_expand::ShrinkAxis;
use niri::layout::{ActivateWindow, AddWindowTarget, LayoutElement as _, Options, SizingMode};
use niri::render_helpers::{RenderCtx, RenderTarget};
use niri_config::{Color, OutputName, PresetSize};
use smithay::backend::renderer::element::RenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::desktop::layer_map_for_output;
use smithay::output::{Mode, Output, PhysicalProperties, Subpixel};
use smithay::utils::{Physical, Point, Size};

use super::{Args, TestCase};
use crate::test_window::TestWindow;

type DynStepFn = Box<dyn FnOnce(&mut Layout)>;

pub struct Layout {
    output: Output,
    windows: Vec<TestWindow>,
    clock: Clock,
    layout: niri::layout::Layout<TestWindow>,
    start_time: Duration,
    steps: HashMap<Duration, DynStepFn>,
}

impl Layout {
    pub fn new(args: Args) -> Self {
        let Args { size, clock } = args;

        let output = Output::new(
            String::new(),
            PhysicalProperties {
                size: Size::from((size.w, size.h)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        let mode = Some(Mode {
            size: size.to_physical(1),
            refresh: 60000,
        });
        output.change_current_state(mode, None, None, None);
        output.user_data().insert_if_missing(|| OutputName {
            connector: String::new(),
            make: None,
            model: None,
            serial: None,
        });

        let options = Options {
            layout: niri_config::Layout {
                focus_ring: niri_config::FocusRing {
                    off: true,
                    ..Default::default()
                },
                border: niri_config::Border {
                    off: false,
                    width: 4.,
                    active_color: Color::from_rgba8_unpremul(255, 163, 72, 255),
                    inactive_color: Color::from_rgba8_unpremul(50, 50, 50, 255),
                    urgent_color: Color::from_rgba8_unpremul(155, 0, 0, 255),
                    active_gradient: None,
                    inactive_gradient: None,
                    urgent_gradient: None,
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let mut layout = niri::layout::Layout::with_options(clock.clone(), options);
        layout.add_output(output.clone(), None);

        let start_time = clock.now_unadjusted();

        Self {
            output,
            windows: Vec::new(),
            clock,
            layout,
            start_time,
            steps: HashMap::new(),
        }
    }

    pub fn sink_stack(args: Args) -> Self {
        let mut rv = Self::new(args);
        let mut config = niri_config::Config::default();
        config.sink.enabled = true;
        rv.layout.update_config(&config);
        for id in 0..3 {
            let window = TestWindow::fixed_size(id);
            rv.add_window(window, Some(PresetSize::Fixed(400)));
        }
        for (delay, id) in [(500, 2), (1000, 1), (1500, 0)] {
            rv.add_step(delay, move |l| {
                l.layout.activate_window(&id);
                l.layout.sink_window();
            });
        }
        rv.add_step(2100, |l| {
            l.layout.pull_window();
        });
        rv.add_step(2200, |l| {
            l.layout.sink_window();
        });
        rv.add_step(3000, |l| {
            l.layout.pull_window();
        });
        rv.add_step(3600, |l| {
            l.layout.unwind_sink_to(&2);
            l.layout.activate_window(&2);
        });
        rv
    }

    pub fn shrink_axes_and_batch_restore(args: Args) -> Self {
        let mut rv = Self::new(args);
        let mut config = niri_config::Config::default();
        config.shrink_expand.enabled = true;
        rv.layout.update_config(&config);
        for id in 0..3 {
            let window = TestWindow::freeform(id);
            window.set_color(match id {
                0 => [0.15, 0.64, 0.41, 1.],
                1 => [0.88, 0.11, 0.14, 1.],
                _ => [0.15, 0.35, 0.8, 1.],
            });
            rv.add_window(window, Some(PresetSize::Proportion(0.3)));
        }
        rv.add_step(2000, |l| {
            l.set_spine(1, ShrinkAxis::Horizontal);
            l.layout.activate_window(&0);
        });
        rv.add_step(5000, |l| l.set_spine(2, ShrinkAxis::Vertical));
        rv.add_step(8000, |l| {
            l.layout.consume_or_expel_window_left(Some(&2));
            l.layout.activate_window(&1);
            l.layout.toggle_column_tabbed_display();
        });
        rv.add_step(11000, |l| {
            for ws in l.layout.workspaces_mut() {
                ws.restore_normal_spines(&[1, 2]);
            }
            l.layout.activate_window(&0);
            l.communicate_windows();
        });
        rv.clock.clear();
        rv.start_time = rv.clock.now_unadjusted();
        rv
    }

    pub fn shrink_floating_move_and_mode(args: Args) -> Self {
        let mut rv = Self::new(args);
        let mut config = niri_config::Config::default();
        config.shrink_expand.enabled = true;
        rv.layout.update_config(&config);
        for id in 0..2 {
            rv.add_window(TestWindow::freeform(id), Some(PresetSize::Fixed(300)));
        }
        rv.add_step(2000, |l| {
            l.layout.set_window_floating(Some(&1), true);
            l.set_spine(1, ShrinkAxis::Horizontal);
            l.layout.activate_window(&0);
        });
        rv.add_step(5000, |l| {
            let (tile, position, _) = l
                .layout
                .active_workspace()
                .unwrap()
                .tiles_with_render_positions()
                .find(|(tile, _, _)| tile.window().id() == &1)
                .unwrap();
            let start = position + Point::from((tile.tile_size().w / 2., tile.tile_size().h / 2.));
            assert!(l.layout.interactive_move_begin(1, &l.output, start));
            assert!(l.layout.interactive_move_update(
                &1,
                (300., 0.).into(),
                l.output.clone(),
                (300., start.y).into(),
            ));
        });
        rv.add_step(7000, |l| l.layout.toggle_window_floating(Some(&1)));
        rv.add_step(9000, |l| l.layout.request_toggle_maximized(&1));
        rv.add_step(11000, |l| {
            l.layout.interactive_move_end(&1);
            l.communicate_windows();
        });
        rv.add_step(13000, |l| {
            l.layout.request_maximized(&1, false);
            l.layout.set_window_floating(Some(&1), true);
            l.communicate_windows();
        });
        rv.clock.clear();
        rv.start_time = rv.clock.now_unadjusted();
        rv
    }

    // Visual fixtures set the internal form directly; seat/timer/ack handling belongs to the
    // compositor's real protocol fixtures. Native ownership and reflow are still used here.
    fn set_spine(&mut self, id: usize, axis: ShrinkAxis) {
        let plans = self
            .layout
            .shrink_plans(&id, 32.)
            .expect("visual Shrink target");
        assert!(plans.get(axis).fits, "visual Shrink axis must fit");
        let tile = self
            .layout
            .workspaces_mut()
            .flat_map(|ws| ws.tiles_mut())
            .find(|tile| tile.window().id() == &id)
            .expect("visual native Tile");
        assert!(tile.set_spine(axis, 32.));
        info!(id, ?axis, "visual spine committed");
        self.layout.update_window(&id, None);
    }

    fn communicate_windows(&mut self) {
        for window in &self.windows {
            if window.communicate() {
                self.layout.update_window(window.id(), None);
            }
        }
    }

    pub fn open_in_between(args: Args) -> Self {
        let mut rv = Self::new(args);

        rv.add_window(TestWindow::freeform(0), Some(PresetSize::Proportion(0.3)));
        rv.add_window(TestWindow::freeform(1), Some(PresetSize::Proportion(0.3)));
        rv.layout.activate_window(&0);

        rv.add_step(500, |l| {
            let win = TestWindow::freeform(2);
            l.add_window(win.clone(), Some(PresetSize::Proportion(0.3)));
            l.layout.start_open_animation_for_window(win.id());
        });

        rv
    }

    pub fn open_multiple_quickly(args: Args) -> Self {
        let mut rv = Self::new(args);

        for delay in [100, 200, 300] {
            rv.add_step(delay, move |l| {
                let win = TestWindow::freeform(delay as usize);
                l.add_window(win.clone(), Some(PresetSize::Proportion(0.3)));
                l.layout.start_open_animation_for_window(win.id());
            });
        }

        rv
    }

    pub fn open_multiple_quickly_big(args: Args) -> Self {
        let mut rv = Self::new(args);

        for delay in [100, 200, 300] {
            rv.add_step(delay, move |l| {
                let win = TestWindow::freeform(delay as usize);
                l.add_window(win.clone(), Some(PresetSize::Proportion(0.5)));
                l.layout.start_open_animation_for_window(win.id());
            });
        }

        rv
    }

    pub fn open_to_the_left(args: Args) -> Self {
        let mut rv = Self::new(args);

        rv.add_window(TestWindow::freeform(0), Some(PresetSize::Proportion(0.3)));
        rv.add_window(TestWindow::freeform(1), Some(PresetSize::Proportion(0.3)));

        rv.add_step(500, |l| {
            let win = TestWindow::freeform(2);
            let right_of = l.windows[0].clone();
            l.add_window_right_of(&right_of, win.clone(), Some(PresetSize::Proportion(0.3)));
            l.layout.start_open_animation_for_window(win.id());
        });

        rv
    }

    pub fn open_to_the_left_big(args: Args) -> Self {
        let mut rv = Self::new(args);

        rv.add_window(TestWindow::freeform(0), Some(PresetSize::Proportion(0.3)));
        rv.add_window(TestWindow::freeform(1), Some(PresetSize::Proportion(0.8)));

        rv.add_step(500, |l| {
            let win = TestWindow::freeform(2);
            let right_of = l.windows[0].clone();
            l.add_window_right_of(&right_of, win.clone(), Some(PresetSize::Proportion(0.5)));
            l.layout.start_open_animation_for_window(win.id());
        });

        rv
    }

    fn add_window(&mut self, mut window: TestWindow, width: Option<PresetSize>) {
        let ws = self.layout.active_workspace().unwrap();
        let min_size = window.min_size();
        let max_size = window.max_size();
        window.request_size(
            ws.new_window_size(width, None, false, window.rules(), (min_size, max_size)),
            SizingMode::Normal,
            false,
            None,
        );
        window.communicate();

        self.layout.add_window(
            window.clone(),
            AddWindowTarget::Auto,
            width,
            None,
            false,
            false,
            ActivateWindow::default(),
        );
        self.windows.push(window);
    }

    fn add_window_right_of(
        &mut self,
        right_of: &TestWindow,
        mut window: TestWindow,
        width: Option<PresetSize>,
    ) {
        let ws = self.layout.active_workspace().unwrap();
        let min_size = window.min_size();
        let max_size = window.max_size();
        window.request_size(
            ws.new_window_size(width, None, false, window.rules(), (min_size, max_size)),
            SizingMode::Normal,
            false,
            None,
        );
        window.communicate();

        self.layout.add_window(
            window.clone(),
            AddWindowTarget::NextTo(right_of.id()),
            width,
            None,
            false,
            false,
            ActivateWindow::default(),
        );
        self.windows.push(window);
    }

    fn add_step(&mut self, delay_ms: u64, f: impl FnOnce(&mut Self) + 'static) {
        self.steps
            .insert(Duration::from_millis(delay_ms), Box::new(f) as _);
    }
}

impl TestCase for Layout {
    fn resize(&mut self, width: i32, height: i32) {
        let mode = Some(Mode {
            size: Size::from((width, height)),
            refresh: 60000,
        });
        self.output.change_current_state(mode, None, None, None);
        layer_map_for_output(&self.output).arrange();
        self.layout.update_output_size(&self.output);
        for win in &self.windows {
            if win.communicate() {
                self.layout.update_window(win.id(), None);
            }
        }
    }

    fn are_animations_ongoing(&self) -> bool {
        self.layout.are_animations_ongoing(Some(&self.output)) || !self.steps.is_empty()
    }

    fn advance_animations(&mut self, _current_time: Duration) {
        let now_unadjusted = self.clock.now_unadjusted();
        let mut run = self
            .steps
            .keys()
            .copied()
            .filter(|delay| self.start_time + *delay <= now_unadjusted)
            .collect::<Vec<_>>();
        run.sort_unstable();
        for delay in &run {
            let now = self.start_time + *delay;
            self.clock.set_unadjusted(now);
            self.layout.advance_animations();

            let f = self.steps.remove(delay).unwrap();
            f(self);
        }

        self.clock.set_unadjusted(now_unadjusted);
        self.layout.advance_animations();
    }

    fn render(
        &mut self,
        renderer: &mut GlesRenderer,
        _size: Size<i32, Physical>,
    ) -> Vec<Box<dyn RenderElement<GlesRenderer>>> {
        self.layout.update_render_elements(Some(&self.output));

        let mut rv = Vec::new();
        let mut ctx = RenderCtx {
            renderer,
            target: RenderTarget::Output,
            xray: None,
        };
        self.layout
            .render_interactive_move_for_output(ctx.r(), &self.output, &mut |elem| {
                rv.push(Box::new(elem) as _)
            });
        self.layout
            .monitor_for_output(&self.output)
            .unwrap()
            .render_workspaces(ctx, true, &mut |elem| rv.push(Box::new(elem) as _));
        rv
    }
}
