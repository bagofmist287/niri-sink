# niri-visual-tests

> [!NOTE]
>
> This is a development-only app, you shouldn't package it.

This app contains a number of hard-coded test scenarios for visual inspection.
It uses the real niri layout and rendering code, but with mock windows instead of Wayland clients.
The idea is to go through the test scenarios and check that everything *looks* right.

## Running

You will need recent GTK and libadwaita.
Then, `cargo run`.

Select the Sink transition scene reproducibly with:

```sh
NIRI_VISUAL_TEST_CASE="Layout - Sink Stack / Reverse / Unwind" cargo run -p niri-visual-tests
```

The offscreen GLES oracle `cargo run --locked --example sink-render-probe` compares native and
projected framebuffer effects across all output transforms, fractional scales and clipped edges,
and checks capture cache isolation and resource ownership. It requires a working EGL device.
It complements the mock scene; neither replaces live Wayland client checks.
