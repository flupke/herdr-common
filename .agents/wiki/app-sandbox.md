# The app sandbox

What `crates/app-sandbox`'s code does not say: the guarantees that span its files, the gotchas
met while building it, and how to test a change to it.

## What keeps it off the user's display

Three layers, in three files; a change to one leans on the others:

- Weston's Wayland socket is `wayland-sandbox`, and no `WAYLAND_DISPLAY` names it: a Wayland
  client looks for `wayland-0`, finds none, and opens on Weston's Xwayland (`lib.rs`).
- The environment drops every inherited variable that names the user's displays, window
  manager, Herdr tools or Claude Code session, and the displays a caller adds
  (`environment.rs`). A caller's variables override the sandbox's defaults, such as
  `BROWSER=true`, never what the isolation stands on.
- In bubblewrap, the user's runtime directory and Herdr directories show empty, so a path typed
  in a sandbox terminal reaches none of their sockets (`bubble.rs`, `user_directories`). Plain
  isolation has no such layer.

A `Bubble` run alone (herdr-bowtie's Herdr server) has the third layer only when its caller
masks `user_directories`, and the second only when the environment it gives `command` or
`launch` leaves out the user's displays and Herdr variables: it passes it on as it is. On
`Network::Host` it also reaches the user's X display's abstract socket and every host port, so
only the second layer keeps a program off their display.

## Reaching it from the host

In bubblewrap the host reaches the sandbox's `/tmp` through `AppSandbox::directory()`, and a
port of its loopback through `AppSandbox::forward` (`forward.rs`).

- A forward is chosen over sharing the host's network (bwrap without `--unshare-net`): shared,
  the sandbox would reach the user's X display by its abstract socket, and any port the user's
  tools listen on.
- Each connection runs socat under nsenter, so an app whose tests forward needs `socat` on its
  dev shell's `PATH`, beside bubblewrap and nsenter.
- A relay's standard input is one end of a socket pair, which socat reads as `FD:0,shut-down`.
  With pipes, or with `STDIO` even on a socket, socat never passes on the end of a server that
  closes first (HTTP's `Connection: close`) until its input ends: the host's client, which
  cannot half-close, waits for an answer's end until socat's `-t`.

## Gotchas

- bwrap reports the holder's PID (`--info-fd`) before it mounts the tmpfs: a path through
  `/proc/<PID>/root` read before the holder's `ready` line still reaches the host's `/tmp`.
- `Command` looks a bare program name up in the *child's* `PATH`: the sandbox's starts with
  its `bin` (`/tmp/bin` in bubblewrap, the host's `/tmp/bin` before nsenter enters), so the
  sandbox resolves `nsenter` with this process's `PATH` first.
- hc gives PIDs as the sandbox's namespace sees them (`hc get t1`'s `pid`), and
  `AppSandbox::processes` gives host PIDs: the last field of `/proc/<host PID>/status`'s
  `NSpid` maps one to the other.
- `herdr-app shot <file>` is written by the window's process, so `<file>` is a path inside the
  sandbox.
- herdr-app 0.1.13 opens its first window at 1280×820 unless `window.json` says otherwise, and
  the kiosk shell resizes it a moment later. Its first frame may still show `Loading…` in the
  sidebar.
- Plain isolation: a process that both clears its environment and leaves its parents (`setsid
  env -i`) escapes the mark and outlives the sandbox; in bubblewrap the kernel ends it.

## Restarts

`AppSandbox::restart` (`restart.rs`) kills as a reboot does. What herdr-app 0.1.13 and hc 0.1.18
do then, which the code leans on:

- The core runs under the window (`herdr-app` → `hc connect` → `hc serve` → `hc hold` →
  programs), yet outlives a `SIGKILL` of the window alone: a reboot kills the window's whole
  tree, then every hc process left.
- Any `hc` call starts a core when none runs, and the window reconnects to whichever core runs.
  After a reboot the kept terminals show `reason: stopped_by_restart` until the new core gives
  them shells.
- After a core kill the window logs `reconnected to the core` in `app.log` (its only sign),
  then loads its layout about half a second later. Until then `herdr-app do` fails ("This
  machine's layout is unavailable", "doc.get: lost the core"), while `herdr-app status` may
  report no error at all. `LAYOUT_PROBE` in `lib.rs` asks for the layout, also at start.
- A kept terminal's shell comes back a second or two after the layout. The window's view of
  it shows `[reconnecting to core…]` and drops the keys typed into it until it reattached,
  within a second of the shell in every run; no event, log line or status field says when.
  Type into a kept terminal through the window only once its pixels show the shell, or reach
  it with `hc send`.
- A process stopped with `SIGSTOP` under nsenter stops nsenter too, which reaps it only once
  continued: `kill_trees` continues each stopped nsenter whose child it killed, until it
  reaped the child, and signals nothing else outside the trees.
- In bubblewrap the killed processes stay zombies of the holder (`cat`, which reaps nothing)
  until the sandbox stops: `processes()` lists them, with no `exe`.

## Testing a change

`make test CRATES=app-sandbox` runs the sandbox in each isolation against the pinned tools;
`make test CRATES=vision-mcp` restarts its core, then reboots and opens the window again, in
each isolation, and drives a probe after each. A change to what keeps it off the user's display
also needs a run beside the user's session: list
the windows of their desktop and which one has the focus, every half second while the tests run,
and check the list never changes. On sway:

```sh
out="${TMPDIR:-/tmp}/windows.json"
snap() { swaymsg -t get_tree | jq -c '[recurse(.nodes[]?, .floating_nodes[]?)
  | select(.pid != null) | {id, name, focused}]'; }
(while true; do snap; sleep 0.5; done) >"$out" & sampler=$!
scripts/dev-shell scripts/check test CRATES=app-sandbox
kill "$sampler"; sort -u "$out" | wc -l   # 1: no window appeared, the focus never moved
```

On X11, `xdotool getwindowfocus` and `wmctrl -l` give the same. A test that breaks the sandbox
on purpose leaves its processes behind in plain isolation: find them by the mark,
`APP_SANDBOX=<directory>` in `/proc/<PID>/environ`, and kill those alone.
