# Sink / Pull (personal fork, experimental)

This feature is specific to the personal `niri-sink` fork. It is disabled by default.
The current N1 implementation manages original window ownership and restoration. It does not yet
draw background projections; it is a logic prototype, not a daily-use visual feature.

Enable it explicitly:

```kdl
sink {
    enabled true
    visible-depth 2
    level 0 scale=0.93 opacity=0.82
    level 1 scale=0.85 opacity=0.45
}
binds {
    Mod+S { sink-window; }
    Mod+P { pull-window; }
}
```

`sink-window` moves the active normal window into the current workspace's background stack.
`pull-window` restores its most recent member. Scrolling windows return through normal insertion
into a new column; floating windows retain their original placement. The client and window ID stay
the same. Explicit focus by ID, focus-previous, MRU confirmation and permitted activation restore
the stack from the top through the requested window. Intermediate members do not acquire focus.
MRU browsing, ordinary directional focus and pointer focus do not restore background members.

Both actions do nothing when disabled, locked, grabbed, conflicting with another input owner or
an interactive move/resize, or when their target is absent. Sink currently rejects fullscreen,
maximized, windowed-fullscreen and transient families. Direct layout edits targeting a sunk ID
do nothing. Closing and inventory queries still work.

Reloading with `enabled false` restores all stacks in LIFO order, including workspaces without
outputs, preserving any existing normal focus. Changing styles retains membership. `visible-depth`
accepts 1–4. Level indices accept 0–3, with no duplicates; scale accepts `(0,1]` and opacity accepts
`[0,1]`. Missing styles retain defaults: `0.93*(0.85/0.93)^depth` and `0.82*(0.45/0.82)^depth`.
These presentation parameters are parsed now; rendering is a separate implementation stage.

The following Overview settings are parsed for the later expansion stage, and do not yet add
Overview behavior:

```kdl
overview {
    sink {
        animate true
        entry-height 24
        tail-height 12
        tail-decay 0.5
    }
}
```

`animate` is the sole switch for additional Sink animation. Entry height accepts 1–256, tail height
0–256, and tail decay 0.01–0.99. There is no Self Blur or mode setting. Native opacity and background
effects are part of the visual implementation contract and have not been verified for Sink yet.

Matching CLI commands are `niri msg action sink-window` and `niri msg action pull-window`.
Their normal IPC success response acknowledges dispatch; inspect inventory/events for actual changes.
See [IPC](IPC.md) for `is_sunk`, `sink_depth` and `sink_count`.
