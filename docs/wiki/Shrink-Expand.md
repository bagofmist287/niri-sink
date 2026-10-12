# Shrink / Expand development configuration

The `shrink/development` branch currently parses the configuration below.
Shrink / Expand commands and input buffering are still under development.
Internal spines now have independent rendering and compositor-owned hit regions. Setting `enabled true` does not yet enable a usable
feature. The installed Sink session has not been replaced by this branch.

```kdl
shrink-expand {
    enabled false
    thickness 32
    shrink-buffer-ms 700
    expand-buffer-ms 1000
    mouse-zone-px 24
    preview-scale 0.75
    animate true
}

layout {
    spine {
        inactive-color "#30343b"
        // active and urgent continue to inherit the effective layout paint.
        text-color "#eeeeee"
    }
}
```

These are experimental defaults. Thickness is in logical pixels, including the
border; it accepts values greater than zero up to 65535. Layout may increase the
effective thickness to fit the border and one physical pixel. Both buffer values
accept integer milliseconds from 0 to 60000; zero requests no timed delay.
Mouse buffering uses distance rather than a timer. `mouse-zone-px` accepts 0 to
65535 logical pixels; zero leaves only the resize boundary as its entry point.
`preview-scale` accepts values greater than zero up to 1. Animation control is
separate from buffering. Vertical spine titles rotate clockwise as one piece;
icons remain upright.

`layout.spine` supports `active-color`, `inactive-color`, `urgent-color`, their
corresponding `*-gradient` nodes, and `text-color`. Colors accept the existing CSS
string or four integer RGBA arguments. Gradients use the existing border syntax,
including alpha, interpolation and workspace-relative positioning.

Unspecified paint inherits the window's effective focus-ring paint when that
ring is enabled, otherwise its effective border paint. A disabled border still
supplies paint. Existing window rules remain part of this source. Urgency takes
priority over visual selection; selecting a spine does not activate its client.

Include files and workspace `layout` sections merge one state at a time. A
color in a later layer replaces an earlier gradient. Within one layer, a gradient
takes priority over a color, matching native border configuration. Explicit
`*-color "inherit"` clears that state's override. Transparent colors remain
explicit colors. `text-color "inherit"` clears the text override. For example:

```kdl
workspace "reading" {
    layout {
        spine {
            inactive-color "inherit"
            active-gradient from="#80c8ff" to="#bbddff" angle=45 in="oklab"
            text-color "inherit"
        }
    }
}
```

Inheritance remains symbolic so the renderer can read the latest layout and
window rules. The parser, paint resolver and internal rendering are tested, including live
paint changes, urgency and explicit inheritance in software EGL. Runtime feature
switching and disable-time restoration still require integration.

The internal spine model also excludes spines from `focus-window-previous`.
They remain in window inventory and keep their collapsed form; when every
window is a spine, this navigation action retains no client focus. Explicit
spine activation and MRU confirmation still await runtime integration.

Internal native movement uses a spine's own outer rectangle as its pointer
anchor and keeps the spine collapsed. Switching a moving spine to floating
placement skips the hidden client's normal floating size request. Focus-based
cursor warping excludes spines in both tiled and floating spaces. These layout
paths are tested. Spine hit regions expose body and edge identity without
forwarding input to the hidden client; drag-time direction adaptation and the
complete operation dispatch still await integration.

Internal spine rendering uses a separate fill and caption, without drawing the
client surface, its popups or background effects. Vertical titles are shaped
once and rotated clockwise; the fallback window icon remains upright. Real app
icon resolution is still pending. Sensitive capture targets replace the inner
fill and caption with black, including when an unblocked caption is cached.
These checks use an explicit software EGL test, not the installed session or a
physical input device. No user Shrink/Expand command is available yet.
