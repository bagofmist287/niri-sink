# niri-experimental session (personal fork)

These resources prepare an independent login entry for the current personal fork.
They are not installed by the ordinary niri build. Native session qualification and
installation are separate steps; adding the files does not certify daily use.

The intended installation paths are:

| Resource | Installed path |
| --- | --- |
| Qualified release binary | `/usr/local/libexec/niri-experimental/niri` |
| `resources/niri-experimental` | `/usr/local/bin/niri-experimental` |
| `resources/niri-experimental-session` | `/usr/local/bin/niri-experimental-session` |
| `resources/niri-experimental.desktop` | `/usr/share/wayland-sessions/niri-experimental.desktop` |
| `resources/niri-experimental.service` | `/usr/local/lib/systemd/user/niri-experimental.service` |
| `resources/niri-experimental-shutdown.target` | `/usr/local/lib/systemd/user/niri-experimental-shutdown.target` |
| Personal configuration | `$XDG_CONFIG_HOME/niri-experimental/config.kdl` (default `~/.config/niri-experimental/config.kdl`) |

The command wrapper selects the independent configuration through `NIRI_CONFIG`.
Explicit `-c` / `--config` arguments still override it. `niri-experimental validate`
validates that configuration; `niri-experimental msg` uses the current `NIRI_SOCKET`,
including when existing helper scripts invoke the distribution's `niri msg`.
The two commands speak the same native IPC protocol for ordinary actions. Sink/Pull
actions require this fork's command or its key bindings.

The login entry uses its own systemd service and shutdown target. It validates the
configuration before importing the login environment and refuses to start from a
live graphical desktop. Start it through the login manager after logging out, or
from a fresh TTY. The service runs `--session`, which selects the native backend,
sets `XDG_CURRENT_DESKTOP=niri`, and publishes the compositor environment. Keeping
the internal desktop name allows the installed niri portal configuration to select
its existing backends. The login entry is still displayed as `niri-experimental`.
Portal UI and screen sharing must be checked in the real experimental session.

Keep niri's native automatic xwayland-satellite integration. Do not add a second
satellite daemon or an experimental satellite service. Personal startup programs
and theme generators also need review for hard-coded paths into the original
configuration. Copy the required includes and scripts into the independent tree;
do not symlink the whole configuration back to `~/.config/niri`.

Installing these files must preserve `/usr/bin/niri`, the original login entries
and their services. Reloading unit definitions must not start or stop a session or
import its environment. Preserve an installation manifest and the qualified
binary/configuration hashes. Exiting the experimental session returns to the login
manager, where the original niri or Umbriel session can be selected. Verify this
recovery path during the real session trial.

Sink is disabled by default in the fork. The experimental personal configuration
can explicitly enable it; see [Sink / Pull](./Sink.md). There is no Self Blur,
and Overview has one switch for additional Sink animation. Native window visual
effects retain their usual configuration.
