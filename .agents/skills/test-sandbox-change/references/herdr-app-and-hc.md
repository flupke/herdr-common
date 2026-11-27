# herdr-app and hc in an app sandbox

Seen on herdr-app 0.1.13 and hc 0.1.18; hc 0.1.20 ran the workspace's tests, restarts
included, and printed the same `hc events` lines. Check a line again after a pin change when the
code leans on it.

## Restarts

`AppSandbox::restart` kills as a reboot does. What herdr-app and hc do then:

- The core runs under the window (`herdr-app` → `hc connect` → `hc serve` → `hc hold` →
  programs), yet outlives a `SIGKILL` of the window alone: a reboot kills the window's whole
  tree, then every hc process left.
- Any `hc` call starts a core when none runs, and the window reconnects to whichever core runs.
  After a reboot the kept terminals show `reason: stopped_by_restart` until the new core gives
  them shells.
- After a core kill the window logs `reconnected to the core` in `app.log`, its only sign, then
  loads its layout about half a second later. Until then `herdr-app do` fails ("This machine's
  layout is unavailable", "doc.get: lost the core"), while `herdr-app status` may report no
  error at all. herdr-app says nothing when it is ready, at start too: ask it for its layout
  until it shows.
- A kept terminal's shell comes back a second or two after the layout. The window's view of it
  shows `[reconnecting to core…]` and drops the keys typed into it until it reattached, within a
  second of the shell in every run; no event, log line or status field says when. **Type into a
  kept terminal through the window only once the window's pixels show the shell, or reach it
  with `hc send`.**
- `hc events` ends with its core: a watch starts again after a core restart or a reboot.

## Processes

- hc gives PIDs as the sandbox's namespace sees them (`hc get t1`'s `pid`);
  `AppSandbox::processes` gives host PIDs. The last field of `/proc/<host PID>/status`'s
  `NSpid` maps one to the other.
- In bubblewrap the killed processes stay zombies of the bubble's init (`cat`, which reaps
  nothing) until the sandbox stops: `processes()` lists them, with no `exe`.
- In bubblewrap, `hc events` runs under nsenter, which forks: killing the nsenter leaves it
  running until the sandbox stops.

## The window

- `herdr-app shot <file>` is written by the window's process: `<file>` is a path inside the
  sandbox.
- herdr-app 0.1.13 opens its first window at 1280×820 unless `window.json` says otherwise, and
  the kiosk shell resizes it a moment later. Its first frame may still show `Loading…` in the
  sidebar.
- `herdr-app status` names the `focused_pane`; `herdr-app ls` maps panes to terminals. A
  mod's pane maps to none.
- In `herdr-app do <command> '<json>'`, `argv` is a JSON array; a string is ignored, and the
  terminal runs a shell.
- herdr-app reloads a mod whose folder was replaced, and `tab.new --mod <mod>` fails for a mod
  missing from `mods/`.

## Input

- After an X input, `herdr-app status` has given the focus the window gave it, in every run so
  far.
- A key sent through hc can reach the program before an input the window is still handing on:
  what must follow the window's input goes through the window too (its clipboard and
  `ctrl+shift+v`, herdr-app's `terminal.paste`).
- herdr-app sends a program the mouse reports of clicks and drags without their modifiers: a
  Ctrl or Alt held through them never shows in the program's reports.
- A paste key pressed while a held drag keeps the left button down leaves a drag on
  herdr-app's own panels (the Spaces list) unable to drop: the release that follows changes
  nothing. A program's drag survives it.
- A click `xdotool` fakes in the sandbox (`mousemove … click 1`) did not reach a mod on
  herdr-app 0.1.13.

## hc's output

- `hc keys` returns once the keys reached the terminal: a paste after it reaches the program
  after them. `hc send` is untested so.
- `hc events --json` prints a snapshot line (`terminals`, each with its `title`), then one
  `{"method": "event", "params": {"terminal", "type", "data"}}` line per change; a title change
  is `type: title` with `data.title`, after the bytes before it.
- hc's `foreground` events at times report a program's arrival after its first frames: no sign
  that a program left or came.
- `hc read <t> --json` gives the screen as `text`, trailing empty rows dropped.
- With `HC_TERMINAL_ID` set, a program that reports its state in OSC 7501 shows in
  `hc get <terminal> --json` as `status` (`{"app": "claude-code", "state": "working"}`).

## Recording

- The root window of Weston's Xwayland is black (rootless Xwayland): `x11grab` records the app's
  window (`-window_id`, from `xdotool search`), never the display.
- nixpkgs' `ffmpeg` has no `x11grab`; `ffmpeg-full` does.

## Drawing

Seen on herdr-app 0.1.13, Weston 16 and the dev shell's Mesa 26.2, on an AMD GPU (amdgpu).

- herdr-app draws with Vulkan, and presents on Xwayland through DRI3 with a GPU's driver. On
  Weston's pixman renderer Xwayland has no DRI3, and the app ends at once: "No GPU adapter found
  that can configure the display surface". On Weston's GL renderer, Xwayland draws on the GPU
  too (glamor) and offers DRI3.
- Weston's GL renderer and Xwayland find Mesa only through `__EGL_VENDOR_LIBRARY_FILENAMES` and
  `GBM_BACKENDS_PATH`: without them Xwayland does not start. Weston takes the first render node
  Mesa drives, and skips one of NVIDIA's own driver.
- `weston.log` names the render node and the GL renderer Weston uses; the app's open files
  (`/proc/<PID>/fd`) name the render node it draws on.
- A screenshot of a still window is the same, byte for byte, shot after shot, on the GPU and in
  software; the two differ from each other in the edges of the text.
