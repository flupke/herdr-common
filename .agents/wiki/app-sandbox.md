# The app sandbox

What `crates/app-sandbox`'s code does not say: the guarantees that span its files, the gotchas
met while building it, and how to test a change to it.

## What keeps it off the user's display

Three layers, in three files; a change to one leans on the others:

- Weston's Wayland socket is `wayland-sandbox`, and no `WAYLAND_DISPLAY` names it: a Wayland
  client looks for `wayland-0`, finds none, and opens on Weston's Xwayland (`lib.rs`).
- The environment drops every inherited variable that names the user's displays, window
  manager, Herdr tools or Claude Code session, and the displays a caller adds
  (`environment.rs`).
- In bubblewrap, the user's runtime directory and Herdr directories show empty, so a path typed
  in a sandbox terminal reaches none of their sockets (`bubble.rs`). Plain isolation has no such
  layer.

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

## Testing a change

`make test CRATES=app-sandbox` runs the sandbox in each isolation against the pinned tools. A
change to what keeps it off the user's display also needs a run beside the user's session: list
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
