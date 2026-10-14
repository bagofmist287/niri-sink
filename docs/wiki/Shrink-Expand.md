# Shrink / Expand

Explicit `focus-window` by ID, MRU confirmation, foreign-toplevel activation, and
policy-approved XDG activation, and clicking its native tab indicator expand a spine
at its current native position. MRU
browsing and cancellation, ordinary navigation, focus follows mouse, and internal
layout selection keep the spine form. XDG token expiry and `on-xdg-activate`
ignore/urgency policy are checked before expansion; input owners and session locks
also retain their existing protection. A permitted activation cancels any pending
selection preview and restores the explicitly requested identity. Focus waits for
the normal restore barrier or its original deadline; navigation or lock invalidates
the pending focus handoff.

A tab indicator selects on press, including hidden spine tabs. Its original pointer,
touch or tablet press serial may retain the focus handoff while that same press owns
the grab; a different interaction still invalidates it. In Overview, tab activation
exits to the target workspace. Ordinary tab hover remains passive.

A newly mapped or reparented transient causes its spine parent to expand without
selecting the parent for focus. While a lock or another interaction prevents this,
the live relationship is checked again on refresh. Child input stays blocked until
the ancestor chain is normal and its restore barriers have settled. Reparenting,
closing either identity, and revoking an existing child grab use the same lifecycle
as the existing Sink transient handling.

Explicit fullscreen, maximize, and windowed fullscreen requests for a spine in an
active native move retain a final mode intent on that identity. The spine and grab
remain intact until native release, when expansion and the final intent apply at
the actual drop position. Repeated toggles use the pending intent; maximize can
remain latent under fullscreen, and windowed fullscreen excludes real fullscreen.
Closing the moving identity discards its intent. Ordinary-window move policy stays
with the native implementation.

Spine content and unconfirmed content previews do not qualify for Wayland idle
inhibition or on-demand VRR. Their normal surface presentation feedback stays intact;
normal restored content follows the native visibility rules. The desktop-wide D-Bus
idle inhibitor keeps its original semantics.

The `shrink/development` branch provides the six commands below when
`enabled true`, with independent spine rendering and compositor-owned hit regions.
Adjacent selection includes a content preview. This experimental feature is disabled
by default; enable it explicitly in the configuration used by a supporting binary.

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
paint changes, urgency and explicit inheritance in software EGL. Configuration
reload supports disable-time restoration as described below.

Spines are excluded from `focus-window-previous`.
They remain in window inventory and keep their collapsed form; when every
window is a spine, this navigation action retains no client focus. Explicit
spine activation and MRU confirmation expand through the permission checks described
above.

Internal native movement uses a spine's own outer rectangle as its pointer
anchor and keeps the spine collapsed. Switching a moving spine to floating
placement skips the hidden client's normal floating size request. Focus-based
cursor warping excludes spines in both tiled and floating spaces. These layout
paths are tested. Spine hit regions expose body and edge identity without
forwarding input to the hidden client. Left-button, touch and pen-tip input use
the spine body as a compositor MoveGrab entry, and its edge as a ResizeGrab entry.
Configured mouse binds keep their native priority. The native drop gap is chosen
before spine shape planning. Both legal axes preserve the moving axis; only the
other axis fitting switches it. If neither fits, the old axis enters the native
overflow allocation. Floating placement preserves the axis. The independent insert
hint uses the planned shape; a new-workspace marker fits that aspect into its
native gap. The moving window keeps its original shape until release.

Release replans against the final native slot and applies compositor geometry to
the same Tile. The pointer retains its native proportion within the new shape;
no client size request or automatic expansion is introduced. Native closest-gap
selection, tie behavior, thresholds and view ownership remain in their original
paths. Current checks cover new-column planning/commit, fallback, revalidation
and real Wayland pointer drops. Native layout fixtures also cover shared and
tabbed columns (including fullscreen/maximized insertion), Overview workspace
creation/reuse, cross-output placement and output loss. Software EGL verifies
both workspace-gap and existing-workspace hints, including Overview zoom.
Layout tests cover live border/thickness and output/workspace overrides during
movement, and compare native edge scrolling against ordinary windows. Physical
input-device calibration and physical interaction remain specific to the running
session and its hardware.

A valid body release below the native 8 px recognition threshold directly expands
the spine through the current-slot allocator. Reaching the threshold forms a move:
a short tiled drag returning before 256 px stays collapsed, and floating movement
uses the native immediate Moving path. The last unframed motion is considered for
spine click classification. Forced ungrab, touch cancellation, pen proximity-out,
changed scene scope or feature disable do not click-expand. A body press during
another preview only cancels that preview and does not expand on release.

Click expansion is queued after the input grab is cleared, then rechecks window,
workspace, output, Overview generation, current hit and seat permission. In Overview,
a valid click exits to the target workspace and expands that window. Edge resizing
accumulates unscaled layout deltas using the current Overview zoom; native client
and modifier resize entries keep their existing coordinate path. Native right-button
floating switches keep the initiating left-button grab, and ordinary CSD horizontal
view-offset recognition remains unchanged. These paths have synthetic pointer,
touch and tablet grab/Wayland evidence, not physical input-device qualification.

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
Paint-only updates preserve their current thickness.

Changing `enabled true` to `enabled false` cancels previews and restores all spines
across every workspace, output, hidden tab and floating space. Workspaces retained
with no outputs are included. The current-slot normal insertion plans share one
native transaction deadline, with all forms prepared before restoration requests.
A moving spine first ends at its native final slot; the existing MoveGrab then
cleans up normally. Ordinary moves continue. An ongoing spine resize is rebased
and continues with its existing grab. The current ordinary focus, or absent focus,
is preserved, and late client commits do not activate the restored books.

The operation snapshot records an ExpandAll batch per affected workspace. Matching
members settle without waiting; other members require processed commit or the
original deadline. Closing targets leave the waiting set. Repeating the same disabled
configuration does not start another batch or extend its deadline. Restores release
only their own staged transaction references when no configure is needed; older
native transactions remain on their original lifecycle. Feature admission stays
disabled while those normal client restores settle.

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

Confirming while the shrink demonstration is still moving preserves its current
pure-color pose and continues toward the committed spine. The logical form, locked
allocation and normal focus handoff change immediately; the remaining visual does
not retain client input or the operation timer. Committed shrinking samples no
client or popup. An immediate expansion starts from this visible pose. Floating
spatial shrink commits the preview's left/top edge anchor before the native size
update, without changing normal-window resize or move behavior. Turning animation
off completes the visual immediately. These paths have layout, real Wayland/grab
and software EGL evidence, with live-client effects also checked in nested sessions.
Nested sessions do not establish physical input-device behavior.

Internal direct and workspace-wide expansion now use the current native slot's
normal size plan. Direct expansion selects that window, but waits for the
processed surface commit or the original 300 ms deadline before client input.
Navigation during that wait cancels the deferred focus handoff. Workspace-wide
expansion includes hidden tabs and floating spines in one transaction, preserving
ordinary focus or the absence of focus. Submitted operation records distinguish
Ready, Deadline and TargetsRemoved; a late commit does not rewrite a terminal
result. These paths are tested with real Wayland fixtures and real IPC sockets.
Confirming a selected spine in Overview exits to its current workspace. Direct
expansion with a focus handoff also exits Overview; batch expansion preserves it.
Reopening Overview during the pending restore cancels the deferred client focus.
Committed restoration has an independent visual transition. A pure-color shape
grows from the current spine rectangle toward the native normal allocation. While
the processed-commit barrier is pending, neither the old client nor its popups are
sampled, including with translucent spine paint. Once the original commit/deadline
barrier releases, content fades in while the shape fades out. Input and deferred
focus follow the barrier, not the visual clock. Turning animation off preserves
the barrier and shows content immediately when it releases. The shape follows live
layout paint and the configured spine overrides; screen-capture block-out remains
effective. Capture caches remain separate from output presentation feedback.
During a selected restoration, subsequent fullscreen or maximized mode changes
also update the projected content's native opacity, backdrop, corners and
decoration transitions. Selecting a spine alone keeps its normal content style.
The native mode resize animation also retains its old-content snapshot and
shader transition before that composition is fitted into the restoring frame.

The transition also applies to batch and disable restoration. Left/top floating
edge restoration retains the opposite edge during the same resize grab. It stores
only the current insertion's visual pose and drops its caches once the transition
ends; it does not retain a pre-shrink size. Software EGL verifies pixels, fractional
scale and capture isolation; this is not physical-device or nested-session evidence.

Nearby expansion pins the current ordinary tiled origin. It lists all direct
spines before same-axis chains, in right/down/left/up and native member order.
Traversal stops at ordinary or differently oriented windows and uses shortest
paths. In a nonempty workspace containing only spines, it visits the full scrolling
start order, including hidden tabs, then floating positions from top to bottom,
left to right, with window-ID ties. A mixed workspace's floating ordinary focus
does not start a tiling search. Repeating the command cycles the current list and
refreshes its timer; passive changes preserve the selected ID and deadline. An
invalid selected identity or search scope cancels rather than choosing another.

Only the selected client is sampled into a compositor-owned content preview.
The estimated normal allocation times `preview-scale` defines the frame; existing
content fits uniformly inside it, centered over the spine. Preview does not
configure, activate or occupy a new layout slot. In scrolling space the native
horizontal viewport can reveal that frame, while the original ordinary focus is
retained. Oversized frames are centered; normal output clipping still applies.
Native resize and scroll-gesture ownership take priority over this reveal. Cancel
brings a still-valid, still-focused ordinary origin back using native column
visibility policy; a different focus or scene scope is preserved.

Confirm submits only the selected identity and continues from its displayed pose
into the current native insertion. The same client/capture caches survive the
handoff, then the normal Tile owns the presentation. Input releases on processed
commit or the original deadline, independently of animation. The held allocation
also releases at that deadline without requiring another client commit, and
presentation resources drop when the visual finishes. Native effects/capture
policy remain in the rendering path. Software EGL and Wayland fixtures cover
geometry, focus, receipts and lifetime; nested live clients additionally exercise
effects, capture privacy, lock handling and presentation feedback.

## Base commands and operation observation

| KDL action | CLI action | Behavior |
| --- | --- | --- |
| `shrink-window` | `shrink-window` | First-axis prediction; repeat changes axis and refreshes the same timer. |
| `expand-nearby-window` | `expand-nearby-window` | Preview/cycle nearby or all-spine candidates; confirm or buffer expiry expands the selected identity. |
| `expand-window id=42` | `expand-window --id 42` | Direct expansion by ID, without a selection preview. |
| `expand-all-windows` | `expand-all-windows` | One current-workspace batch including hidden tabs and floating spines. |
| `confirm-shrink-expand` | `confirm-shrink-expand` | Revalidate and submit the current preview. |
| `cancel-shrink-expand` | `cancel-shrink-expand` | Cancel the current preview. |

Opposite Expand input during a Shrink preview only cancels that preview, including
nearby, explicit-ID and batch commands. Shrink input during nearby selection only
cancels it.
Escape also cancels. The action dispatcher checks seat ownership before the
native common grab side effects, preserving pointer/touch/IME constraints.
Spatial shrinking and deferred press activation also respect tablet grabs while
pointer or touch owns its native resize/press; that device-specific exception does
not permit taking input from a tablet.

For example, on a binary built from this branch:

```sh
niri msg --json action shrink-window
niri msg --json action expand-nearby-window
niri msg --json action --wait expand-window --id 42
niri msg --json action --wait=2000 expand-all-windows
niri msg --json shrink-expand-state --operation-id 7
```

The six actions return `ShrinkExpandAction` receipts with operation/workspace IDs,
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
