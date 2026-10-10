# Sink / Pull (personal fork, experimental)

This feature is specific to the personal `niri-sink` fork. It is disabled by default.
The N2 implementation keeps the original live window and projects it behind normal windows.
It is an experimental visual prototype; full system compatibility and the expanded Sink Overview
remain later stages.

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

`sink-window` moves the active window into the current workspace's background stack.
`pull-window` restores its most recent member. Scrolling windows return through normal insertion
into a new column; floating windows retain their original placement. The client and window ID stay
the same. Explicit focus by ID, focus-previous, MRU confirmation and permitted activation restore
the stack from the top through the requested window. Intermediate members do not acquire focus.
MRU browsing, ordinary directional focus and pointer focus do not restore background members.

Both actions do nothing when disabled, locked, grabbed, conflicting with another input owner or
an interactive move/resize, or when their target is absent. Sink currently rejects transient families.
Direct layout edits targeting a sunk ID do nothing. Closing and inventory queries still work.

Fullscreen, maximized and windowed-fullscreen windows retain their native state while sunk.
Client fullscreen/maximize requests still receive native configures without pulling the window or
giving it focus. The projection remains behind normal foreground windows. Fullscreen's native
backdrop is part of the projected object, rather than a separate foreground backdrop.
Pull preserves the maximized fallback underneath fullscreen. A window that entered these states
from floating returns to its stored floating placement when those states are unset. When an
initially fullscreen window has no known normal size, unfullscreen sends a zero size hint for the
client to choose; Pull then applies the saved normal layout policy.

Visible sunk windows do not activate per-window on-demand VRR or application idle inhibitors,
including inhibitors on their subsurfaces and tracked popups. Pull restores normal eligibility;
the existing visibility and window-rule requirements still apply. System-level FDO idle inhibition
and always-on output VRR keep their usual behavior. Render visibility and presentation/frame
callbacks remain available to live projections. Native TTY/DRM behavior is still awaiting validation.

Reloading with `enabled false` restores all stacks in LIFO order, including workspaces without
outputs, preserving any existing normal focus. Changing styles retains membership. `visible-depth`
accepts 1–4. Level indices accept 0–3, with no duplicates; scale accepts `(0,1]` and opacity accepts
`[0,1]`. Missing styles retain defaults: `0.93*(0.85/0.93)^depth` and `0.82*(0.45/0.82)^depth`.
The projection fits inside the working area, stays centered and scales the complete native object.
Depth scale does not resize the client. Members beyond `visible-depth` stay owned by the workspace
but release their projection resources after fading out. Pull reveals them again.

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
effects are retained: native window and popup opacity, client alpha, blur (xray and non-xray),
saturation, noise, effect regions, subsurfaces, borders, corners and shadows. Depth opacity is
applied once to the combined object. Each capture target has its own cache and retains block-out rules.

Pull immediately schedules normal layout insertion and its native resize transaction while the
projection returns. Input waits for the visual transition and a processed commit acknowledging the
required resize or native state change; an ack alone is insufficient. A matching already committed
size and mode takes the fast path. Another native state/size change during restoration retargets
the required commit while retaining the original deadline.
The original transaction deadline bounds an unresponsive client (normally 300 ms); after it expires,
niri uses the existing degraded resize behavior. Changing focus, locking, unmapping or invalidating
the restore context cancels the old focus plan, so a late commit cannot steal focus. Selecting the
window again follows normal navigation and still respects any outstanding commit barrier.

Matching CLI commands are `niri msg action sink-window` and `niri msg action pull-window`.
Their normal IPC success response acknowledges dispatch; inspect inventory/events for actual changes.
See [IPC](IPC.md) for `is_sunk`, `sink_depth` and `sink_count`.
