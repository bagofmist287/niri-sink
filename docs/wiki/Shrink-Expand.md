# Shrink / Expand development configuration

The `shrink/development` branch provides the five base commands below when
`enabled true`, with independent spine rendering and compositor-owned hit regions.
Adjacent selection, complete expansion visuals and disable-time restoration
remain under development. This is a partial development candidate;
the installed Sink session has not been replaced by this branch.

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
once and rotated clockwise; the fallback window icon remains upright. App icons are resolved from the desktop entry’s `Icon` value and the current
GTK/GSettings icon theme, with recursive inheritance and hicolor/pixmaps fallback.
PNG and SVG are decoded by the installed GdkPixbuf loaders; unsupported or missing
icons use the window fallback. App icon colours and aspect ratio are preserved. Sensitive capture targets replace the inner
fill and caption with black, including when an unblocked caption is cached.
These checks use an explicit software EGL test, not the installed session or a
physical input device. The base commands below use this renderer.

The layout now carries the parsed behavior settings through global configuration
updates. Changing `thickness` reallocates existing internal spines in tiled,
floating and native Moving ownership, without requesting that size from the
hidden client. It also updates workspaces retained while no output is present.
Paint-only updates preserve their current thickness. `enabled false` restoration
still requires integration; existing spines can be expanded with the base commands
while the feature is enabled.

Icon source images are cached separately from title shaping. A changed title or
text colour does not reread the app icon. A new app ID/physical icon size or layout
configuration update invalidates the source cache, including failed lookups.
GIO and GdkPixbuf are now direct build dependencies at the already locked versions;
no additional package version was upgraded. This resolver does not register the
optional xdg-toplevel-icon protocol. Physical device and installed-session checks
remain pending.

The internal timed Shrink preview retains its normal layout allocation and focus.
It grays the content and draws a separate solid ghost using the effective spine
paint; a gradient supplies its midpoint through the native interpolation shader.
Changing axis starts from the ghost's current animated shape. Disabling animation
changes that shape immediately without changing the buffer timer. Cancellation
removes the preview; a successful form commit also clears it. Output, screencast
and screenshot rendering use separate caches, and capture passes preserve the
output's presentation-feedback mapping. CPU, Wayland fixture and software EGL
checks cover these paths; normal-window spatial buffering is described below.

Internal direct and workspace-wide expansion now use the current native slot's
normal size plan. Direct expansion selects that window, but waits for the
processed surface commit or the original 300 ms deadline before client input.
Navigation during that wait cancels the deferred focus handoff. Workspace-wide
expansion includes hidden tabs and floating spines in one transaction, preserving
ordinary focus or the absence of focus. Submitted operation records distinguish
Ready, Deadline and TargetsRemoved; a late commit does not rewrite a terminal
result. These paths are tested with real Wayland fixtures and real IPC sockets.
Complete expansion visuals and restoration of all workspaces on disable remain
under development.

## Base commands and operation observation

| KDL action | CLI action | Behavior |
| --- | --- | --- |
| `shrink-window` | `shrink-window` | First-axis prediction; repeat changes axis and refreshes the same timer. |
| `expand-window id=42` | `expand-window --id 42` | Direct expansion by ID, without a selection preview. |
| `expand-all-windows` | `expand-all-windows` | One current-workspace batch including hidden tabs and floating spines. |
| `confirm-shrink-expand` | `confirm-shrink-expand` | Revalidate and submit the current preview. |
| `cancel-shrink-expand` | `cancel-shrink-expand` | Cancel the current preview. |

`expand-nearby-window` is not available yet. Opposite Expand input during a Shrink
preview only cancels that preview, including explicit-ID and batch commands.
Escape also cancels. The action dispatcher checks seat ownership before the
native common grab side effects, preserving pointer/touch/IME constraints.

For example, on a binary built from this branch:

```sh
niri msg --json action shrink-window
niri msg --json action --wait expand-window --id 42
niri msg --json action --wait=2000 expand-all-windows
niri msg --json shrink-expand-state --operation-id 7
```

The five actions return `ShrinkExpandAction` receipts with operation/workspace IDs,
actual targets, Previewing/Submitted/Cancelled/NoChange and `waiting_for_clients`.
Ordinary actions retain `Handled`. Submitted asserts native form/allocation, not a
client response. `--wait` subscribes to the event stream and includes its terminal
record, with a 5000 ms default bound or an explicit 1–60000 ms bound. A Deadline
result is reported as Deadline. Unknown or expired IDs stop observation with an
error. A no-op creates no operation to wait for.

`ShrinkExpandState` queries and initial `ShrinkExpandChanged` events include the
current preview, submitted batches and the most recent 64 terminal records, closing
the receipt/subscription race. Snapshots update on operation revisions and outcomes,
without animation-frame countdown events. Window inventory includes `shrink_state`
(Normal/Spine) and `shrink_axis` only for a spine. Previews keep Normal; actual form
changes use existing window-change events and keep the same window ID. Client
`window_size` and native `tile_size` retain their distinct meanings.

Keyboard `set-column-width`, `set-window-width` and `set-window-height` commands
also enter timed Shrink when the actual focused normal window is already at the
native allocation's minimum and a further decrease is requested on that axis.
The first ordinary resize reaching the minimum remains an ordinary resize.
Shared columns, hidden-tab constraints and effective borders participate in this
read-only boundary query. A targeted size command only triggers Shrink when its ID
is the actual focused window.

During a timed preview, another decrease on the selected axis refreshes the same
operation/timer and keeps that axis. A grow request on that axis cancels the preview
and executes the native size command. Zero adjustments do not refresh the timer.
Changes on the other axis continue as native resize and update the ghost's shape
without restarting the buffer deadline. These paths retain the original client
identity/focus context; they do not expand an already committed spine. Existing
size-command IPC replies remain `Handled`; operation state/events expose any
boundary-triggered preview.

## Normal-window spatial resize buffering

The existing ResizeGrab reads its raw dimension request before native clamping.
For a normal window, moving into the minimum-plus-`mouse-zone-px` region continues
the drag with gray content and a solid virtual window. The normal allocation stays
at that legal entry boundary; the virtual extent follows the raw pointer request,
with spine thickness as its lower limit. There is no Shrink timer. Zero zone means
only a request strictly beyond the native minimum enters the preview.

Leaving the region removes the preview and resumes the same native resize. A valid
release submits the spine immediately, then uses the usual normal-window focus
handoff. Forced ungrab, touch cancellation and tablet proximity-out do not commit.
Escape or an opposite command cancels only the preview; resizing continues and
cannot immediately reenter that same region until the pointer leaves it first.
This does not add generic Escape cancellation to ordinary ResizeGrab or MoveGrab.

Edge signs, centered horizontal doubling and the topmost scrolling tile's top-edge
restriction use the shared native calculation. Virtual left/top-edge resizing keeps
the opposite edge anchored, and centered resizing keeps the virtual center. Only a
spatial candidate flushes a final unframed motion at release; ordinary and feature-off
resizes retain native frame batching.

Pointer/Wayland fixtures and software EGL verify these normal-window paths. Touch
and tablet lifecycle hooks are connected but their device-specific behavior still
needs qualification.

## Resizing a spine

Dragging along the free axis keeps the window collapsed. Growing along its locked
axis inserts its normal content at the current slot, using this motion's sizing
intent and native client/column constraints. It retains the window ID and native
owner, without restoring a saved pre-shrink size.

The same ResizeGrab continues after expansion. Its size and cumulative-delta origin
are rebased at the form change so the next motion continues from the new normal
allocation. Floating left/top resizing retains the opposite edge. The expanded
client remains behind the existing finite commit/deadline input barrier; later
resize requests update that allocation without extending the original deadline.
Once the barrier settles, dragging smaller can enter spatial Shrink again, and a
valid release collapses the window. Keyboard dimension commands keep their
existing focus requirements and do not expand a committed spine.

Selecting the restored scrolling column preserves only this owned resize; ordinary
column navigation retains its usual cancellation rule. Native MoveGrab thresholds,
detachment and insertion remain unchanged. A last unframed locked-axis grow is
processed at release, while ordinary and feature-off release batching is preserved.
