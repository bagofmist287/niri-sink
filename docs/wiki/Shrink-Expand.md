# Shrink / Expand development configuration

The `shrink/development` branch currently parses the configuration below.
Shrink / Expand commands, input buffering and independent spine rendering are
still under development. Setting `enabled true` does not yet enable a usable
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
window rules. The parser and paint resolver are tested; actual rendered colors,
reload damage and disable-time restoration still require runtime integration.
