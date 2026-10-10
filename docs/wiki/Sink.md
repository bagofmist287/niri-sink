# Sink / Pull (personal fork, experimental)

This feature is specific to the personal `niri-sink` fork. It is disabled by default.
The implementation keeps the original live window and projects it behind normal windows.
It remains experimental. Overview expands the background stack and supports local Sink/Pull;
native hardware and personal daily-use confirmation are in progress. Independent login
resources are described in [niri-experimental session](./Experimental-Session.md).

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

In Overview, clicking or tapping an exposed, visible background projection restores the stack
through that window and closes Overview onto its workspace. Hover and press remain passive;
confirmation happens on release and still waits for the normal Pull input barrier. Moving beyond
the tap threshold, cancelling a touch, adding another touch or losing the selection context cancels
confirmation. Background projections do not receive normal client pointer or touch input.
Overview centers previews horizontally and shifts the foreground down with the stack. The first
`visible-depth` members expose top strips for selection. Older members have one bounded visual tail
hint; that hint does not select an arbitrary hidden window. Hidden members remain accessible through
MRU or explicit window ID. Normal, floating and fullscreen foreground windows keep their Z priority.
Expansion fits each native source without resizing it. Space is reserved for empty workspaces too,
so the first Sink does not change the other workspaces' spacing.
Dragging an ordinary window to another workspace, output or the gap for a new workspace keeps
the original Sink stacks in place. Drag-and-drop hold over a background entry may select its
workspace and close Overview, but never recalls that entry.

Sink and Pull keep Overview open. They start from the current displayed pose, including when
reversing an unfinished restore, and move only that workspace's stack and foreground. Local
reflow uses the configured `window-movement` animation with its additional progress bounded to
`[0,1]`, so it stays inside the reserved space. Opening and closing still use niri's one existing
Overview progress, including its spring/gesture behavior. Turning the additional animation off
finishes the local properties immediately; the native restore animation and processed-commit input
barrier remain in force. Clicking a background entry still starts closing Overview immediately.
Changing output size, scale or the expansion budget rebuilds the additional local coordinates
inside the new allocation, without replaying Overview opening or restarting the native restore
and processed-commit barrier.

Both actions do nothing when disabled, locked, under an interaction grab, conflicting with another input owner or
an interactive move/resize, or when their target is absent. Sink rejects windows already in transient families. When a newly mapped child or a changed parent
relationship refers to a sunk parent, niri restores through that parent when input ownership permits.
The child cannot receive input until its ancestor chain has completed restoration, including any
required processed resize commit. Locks and grabs defer restoration; reparenting or closing the
parent clears obsolete blocking. A late relationship also revokes an interaction already held
by the blocked child: pointer presses remain owned until release without client delivery, touch
is cancelled, and tablet tools leave proximity. Grabs held by unrelated windows are preserved. This does not sink a whole transient group.
Input-method keyboard grabs (for example, Fcitx5 with Kitty) do not block Sink/Pull or
explicit recall. They remain active while the normal focus change is processed; this does not
disable the input method. Popup, pointer, touch and other keyboard grabs retain their protections.
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

Workspace moves and output disconnection keep independent stacks attached to their workspace
identities. Removing all outputs preserves the windows for reconnection; native output enter/leave
notifications and size/scale changes still follow niri policy.

Reloading with `enabled false` restores all stacks in LIFO order, including workspaces without
outputs, preserving any existing normal focus. Changing styles retains membership. `visible-depth`
accepts 1–4. Level indices accept 0–3, with no duplicates; scale accepts `(0,1]` and opacity accepts
`[0,1]`. Missing styles retain defaults: `0.93*(0.85/0.93)^depth` and `0.82*(0.45/0.82)^depth`.
The projection fits inside the working area, stays centered and scales the complete native object.
Depth scale does not resize the client. Members beyond `visible-depth` stay owned by the workspace
but release their projection resources after fading out. Pull reveals them again.

The additional Overview presentation can be configured separately:

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

`animate` is the sole switch for additional Sink animation. When enabled, the additional geometry
and depth opacity follow niri's existing Overview progress. When disabled, they switch immediately
to the same endpoint; niri's own Overview animation still follows its normal settings. Entry height
accepts 1–256, tail height 0–256, and tail decay 0.01–0.99. These heights are final Overview logical
pixels. Small outputs limit the total budget; short sources can reduce the exposed strips together.
Setting tail height to zero hides the tail hint. There is no Self Blur or mode setting. Native opacity and background
effects are retained: native window and popup opacity, client alpha, blur (xray and non-xray),
saturation, noise, effect regions, subsurfaces, borders, corners and shadows. Depth opacity is
applied once to the combined object. Each capture target has its own cache and retains block-out rules.
Native opacity and background-effect rules can be reloaded while Overview is opening, closing
or reflowing a local stack. The additional animation switch does not replace those native rules;
updated client effect regions and changes underneath the projection use the current lower scene.
Output recording follows the composed desktop; single-window recording keeps the native source
without the depth transform. While locked, window recordings clear their buffers and output
recordings follow the lock screen. Unlocking resumes recording through the existing stream.

Pull immediately schedules normal layout insertion and its native resize transaction while the
projection returns. Input waits for the visual transition and a processed commit acknowledging the
required resize or native state change; an ack alone is insufficient. A matching already committed
size and mode takes the fast path. Another native state/size change during restoration retargets
the required commit while retaining the original deadline.
Moving a restoring window or column to another workspace on the same output preserves its
current displayed pose, including during Overview opening or closing. The move keeps the
original restore animation progress and commit deadline; it does not acknowledge client commits.
Moving a restoring window or column to another output discards local Overview coordinates and
projection buffers from the old output. Changing the viewport maps the native restore origin's
center and fit into the new viewport, including workspace moves and output resize/rotation.
Local Overview properties rebuild in the destination's allocation; the native restore clock,
alpha and processed-commit barrier remain intact. Native window effects keep their usual rules.
The original transaction deadline bounds an unresponsive client (normally 300 ms); after it expires,
niri uses the existing degraded resize behavior. Changing focus, locking, unmapping or invalidating
the restore context cancels the old focus plan, so a late commit cannot steal focus. Selecting the
window again follows normal navigation and still respects any outstanding commit barrier.

Matching CLI commands are `niri msg action sink-window` and `niri msg action pull-window`.
Their normal IPC success response acknowledges dispatch; inspect inventory/events for actual changes.
See [IPC](IPC.md) for `is_sunk`, `sink_depth` and `sink_count`.
