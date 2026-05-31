# Wayland Compatibility Matrix

## TL;DR

**Libre VMM is Wayland-native.** No XWayland required. We build on
[`eframe`/`egui`](https://github.com/emilk/egui) 0.30 on top of
[`winit`](https://github.com/rust-windowing/winit), which has first-class
support for the Wayland protocol via `wayland-client`. Where VMware Workstation
falls back to XWayland (and glitches under fractional scaling, multi-monitor
DPI, and certain compositors), Libre VMM speaks Wayland directly.

This document tracks which compositors we have verified the app on, lists
known issues, and provides a contributor sign-off checklist.

---

## Why this matters

VMware Workstation ships as an X11/GTK2 app and runs through XWayland on
modern Linux desktops. Common user complaints:

- Blurry rendering on HiDPI displays under fractional scaling.
- Cursor jumping or stuck modifier keys when input passes through XWayland's
  XInput2 layer.
- Window decorations drawn by the legacy GTK2 client-side codepath, which
  ignores the host compositor's preferred theme.
- Per-monitor DPI not propagated across X11 root window boundaries.

Libre VMM avoids each of these by being a Wayland-native client. The GUI
talks to the compositor through `wl_surface`, `xdg_toplevel`, and
`wp_fractional_scale_v1` directly. SPICE input passthrough uses Wayland's
`wl_pointer` / `wl_keyboard` events for crisp HID delivery to the guest.

---

## Verified compositors

Status legend: ☑ tested and working / ⚠️ partial or known issue / ⚪ untested.

| Compositor | Family | Status | Last verified on |
|---|---|---|---|
| Mutter (GNOME 45+) | wl-shell / xdg-shell | ⚪ | — |
| Mutter (GNOME 46) | wl-shell / xdg-shell | ⚪ | — |
| Mutter (GNOME 47) | wl-shell / xdg-shell | ⚪ | — |
| KWin (KDE Plasma 6) | wl-shell / xdg-shell | ⚪ | — |
| Sway (wlroots) | xdg-shell / layer-shell | ⚪ | — |
| Hyprland | wlroots-based | ⚪ | — |
| River | wlroots-based | ⚪ | — |
| Cosmic (System76) | smithay-based | ⚪ | — |
| Weston (reference) | reference compositor | ⚪ | — |
| Mir (Ubuntu Frame) | mir-based | ⚪ | — |

> Mutter is egui's primary Wayland test target upstream, so we expect that
> to be the smoothest experience. wlroots-based compositors (Sway, Hyprland,
> River) share a code path that should be uniformly well-supported. KWin and
> Cosmic each have their own quirks worth tracking. Weston is the protocol
> reference — useful for distinguishing "egui bug" from "compositor bug".

To add a verified entry, contributors should change the status emoji and
record the distro / version where the run succeeded, e.g.:

```
| Sway (wlroots) | xdg-shell / layer-shell | ☑ | Arch Linux, Sway 1.10, 2026-04 |
```

---

## Known issues

> Currently empty — please add findings here as you verify. A "known issue"
> belongs in this section when it reproduces on a clean install of an
> upstream release of a compositor with default settings.

<!-- Example entry, remove once a real one is added:
- **KWin 6.0 / Plasma 6.0 only**: window resize from the corner can leave a
  one-frame flicker when fractional scaling is enabled. Fixed in KWin 6.1.
  Workaround: set integer scaling, or upgrade KWin.
-->

---

## How to verify Wayland (not XWayland) is being used

The fastest way is to enable Wayland's debug logging and look at the
protocol stream:

```bash
WAYLAND_DEBUG=1 ./vmm-gui 2>&1 | head -40
```

If you see lines like `wl_registry@2`, `wl_compositor@N`, or
`xdg_wm_base@N`, the app is talking to the Wayland compositor directly.
If instead you see no Wayland traffic and `xeyes` could plausibly attach to
the window, you are on XWayland.

You can also probe the running window:

```bash
# If xprop succeeds, you're on X11 or XWayland.
# If xprop errors out with "unable to open display" or no window matches,
# you're on native Wayland (xprop is X11-only).
xprop -id $(xdotool getactivewindow) 2>/dev/null \
  && echo "X11/XWayland" \
  || echo "Native Wayland (or xdotool can't see the window)"
```

A third option is to inspect the environment of the running process:

```bash
pgrep -f vmm-gui | head -1 | xargs -I{} cat /proc/{}/environ \
  | tr '\0' '\n' | grep -E '(WAYLAND_DISPLAY|XDG_SESSION_TYPE|DISPLAY)'
```

`XDG_SESSION_TYPE=wayland` plus a populated `WAYLAND_DISPLAY` is what you
want to see. `DISPLAY=:0` may also be set — that's the XWayland socket,
which we ignore in native mode.

---

## Force XWayland fallback (debugging only)

If you suspect a Wayland-specific regression, you can force the app down
the X11 codepath:

```bash
# Force the main eframe/winit window onto X11/XWayland:
WINIT_UNIX_BACKEND=x11 ./vmm-gui

# Force GTK file dialogs (rfd) onto X11/XWayland:
GDK_BACKEND=x11 ./vmm-gui

# Both at once:
WINIT_UNIX_BACKEND=x11 GDK_BACKEND=x11 ./vmm-gui
```

This is for diagnostics only — please file a bug report including which
backend reproduces the issue and which does not. We do not recommend
running this way day-to-day; XWayland exists in this matrix as a
comparison baseline, not as a supported configuration.

---

## SPICE under Wayland

The SPICE remote-display console is rendered by `spice-gtk`, which is a
GTK3/4 widget. spice-gtk has its own Wayland support and renders directly
to a `wl_surface` when the GTK app is running on Wayland.

Known interactions:

- **Cursor capture** — under Wayland the compositor decides when to grab
  the pointer (via `zwp_pointer_constraints_v1`). spice-gtk uses this
  protocol when available; if your compositor doesn't implement it, you
  will not get relative-mouse mode for the guest. wlroots, Mutter, KWin,
  and Cosmic all support it.
- **Keyboard layout** — Wayland sends keymaps over `wl_keyboard`. SPICE
  passes scan codes; the guest still needs a matching layout. This is
  unchanged from the X11 case.
- **OpenGL acceleration** — `spice-gtk` with OpenGL uses EGL on Wayland.
  Some compositors (notably older Weston builds) reject `dmabuf` formats
  the way spice-gtk negotiates them; fall back to software rendering by
  unsetting `SPICE_GL`.

The console widget is embedded into the egui surface via `WidgetWithState`
on the GUI side — see `vmm-gui/src/spice_console.rs` for the bridge.

---

## HiDPI under Wayland

egui receives the surface scale factor through winit's
`WindowEvent::ScaleFactorChanged`. We feed this into `pixels_per_point`
in `vmm-gui/src/app.rs` via the egui context, so a 200 % scale display
gets 2.0× UI scaling automatically.

What works well:

- **Integer scaling** (1.0, 2.0, 3.0) — perfectly crisp; this is the easy case.
- **Fractional scaling** via `wp_fractional_scale_v1` (Mutter 43+, KWin 5.27+,
  Sway 1.9+, Hyprland 0.30+) — egui receives the exact fractional value and
  draws at the native pixel grid.

Edge cases to be aware of:

- **Per-monitor DPI** — when the user drags the window between a 1.0× and a
  2.0× monitor, winit fires a fresh `ScaleFactorChanged`. egui re-lays-out
  on the next frame. There is a single-frame flicker during the swap; we
  consider this acceptable. If a contributor sees a persistent stale-scale
  bug, please file it under Known Issues above with reproduction steps.
- **Compositor reports integer-only when the user wanted fractional** —
  GNOME 44 and earlier did this for legacy reasons. Workaround on the user
  side: enable the experimental fractional scaling feature in GNOME's
  hidden settings, or upgrade to GNOME 45+.

---

## Testing checklist for contributors

Copy this block into your sign-off when you verify a compositor. Tick what
you tested.

```
Compositor:      _______________________
Version:         _______________________
Distro:          _______________________
Date:            YYYY-MM-DD

- [ ] App launches without error
- [ ] Window decoration / titlebar correct (server-side or client-side,
      whichever the compositor prefers)
- [ ] Window can be resized, maximized, and tiled
- [ ] HiDPI scaling correct on the launch monitor
- [ ] File dialogs (rfd) open and return a valid path
- [ ] SPICE / VNC console renders a guest display
- [ ] Keyboard input passes through to the guest (try a non-Latin layout)
- [ ] Mouse moves and clicks pass through to the guest
- [ ] Drag-and-drop a file into the console (SPICE guest agent)
- [ ] Clipboard sync host -> guest (SPICE)
- [ ] Clipboard sync guest -> host (SPICE)
- [ ] Full-screen mode (F11 or window menu) works and returns cleanly
- [ ] Multi-monitor: drag the window between two monitors with different DPI
- [ ] Multi-monitor: drag the window between two monitors with different
      refresh rates (vsync stays sane)
- [ ] Looking Glass quick-launch button works (if you have a GPU passthrough
      VM and the Looking Glass client installed)
- [ ] App exits cleanly via the close button (no zombie surfaces in the
      compositor)
```

When you're done, open a PR that:

1. Updates the **Verified compositors** table with your tick and the
   distro/version/date.
2. Adds any new entries to **Known issues** if you found regressions.
3. (Optional) Attaches a short screen recording — particularly useful for
   HiDPI and multi-monitor verification.

---

## Reporting a Wayland bug

Please include in any Wayland-related bug report:

1. Distro and compositor version (`gnome-shell --version`, `kwin_wayland --version`,
   `sway -v`, etc.).
2. Output of `echo $XDG_SESSION_TYPE $WAYLAND_DISPLAY`.
3. Output of `WAYLAND_DEBUG=1 ./vmm-gui 2>&1 | head -200` (please redact any
   home paths if you'd rather not share them).
4. Whether the issue reproduces under `WINIT_UNIX_BACKEND=x11 ./vmm-gui`.

If the bug only reproduces on Wayland and not on XWayland, that is
extremely useful information — please say so explicitly.

---

## Upstream tracking

Wayland support in our stack is driven by three projects we depend on:

- **winit** — handles Wayland surface creation, input, scale factor. Tracking
  branch: `main`. Wayland bugs filed upstream at
  https://github.com/rust-windowing/winit/issues with the `Wayland` label.
- **egui / eframe** — renders the UI on top of winit. Wayland-specific
  concerns at https://github.com/emilk/egui/issues filtered by `wayland`.
- **rfd** (file dialog) — uses xdg-desktop-portal under the hood on
  Wayland sessions. Issues at https://github.com/PolyMeilex/rfd/issues.

If you find a Wayland bug in Libre VMM, please first check whether it
reproduces in a minimal eframe template app. If so, the right place to
report is upstream; we will track it from there.

---

*This document is maintained as part of Wave 12 of the Libre VMM roadmap.
"Lead, don't follow" — VMware ships X11 in 2026; we ship native Wayland.*
