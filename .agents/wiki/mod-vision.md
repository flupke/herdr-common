# The herdr-mod-vision skill

What `skills/herdr-mod-vision` and the `app-sandbox` command (`crates/app-sandbox-cli`) do not
say themselves: why they work as they do, and how to test a change to them.

## How it fits

Each call of the skill's `app-sandbox` script enters the checkout's dev shell and `cargo run`s
the command: about a second. The holder that `start` leaves runs in the dev shell's
environment, so `run` finds the dev shell's tools (`jq`, `ffmpeg`). Bubblewrap ends the sandbox
when the holder dies, `SIGKILL` included.

The script builds what the checkout that holds the skill has checked out: installed from the
main checkout, a broken change there breaks every agent's `app-sandbox`, and a build that
waits for cargo's lock waits silently (`--quiet`).

## Gotchas

- The user's `~/.config/herdr-app` shows empty in bubblewrap, and `/tmp`, which holds Claude
  Code's scratchpad, is the sandbox's own tmpfs: `mod` copies a mod in, and `shot` writes its
  file from outside. A path given to `run` or `herdr-app` is a path inside.
- Claude Code's Read resolves `/proc/<PID>/root/...`, the sandbox's directory from outside, to
  `/...`, and finds nothing: the skill shoots to the scratchpad, and copies files out.
- Without `XDG_RUNTIME_DIR`, the sockets go in `/tmp/app-sandbox-<uid>`, never in `TMPDIR`:
  each entry in the dev shell sets another `TMPDIR`.
- `stop` waits for the `run`s in flight: a program that never ends holds the sandbox, and the
  commands sent after the `stop`, until it is killed.
- The root window of Weston's Xwayland is black (rootless Xwayland): `x11grab` records the app's
  window (`-window_id`, from `xdotool search`), never the display. nixpkgs' `ffmpeg` has no
  `x11grab`; `ffmpeg-full` does.
- A click `xdotool` fakes in the sandbox (`mousemove … click 1`) did not reach the app on
  herdr-app 0.1.13: commands are the skill's hands.
- herdr-app reloads a mod whose folder `mod` replaced, and `tab.new --mod <mod>` fails for a mod
  missing from `mods/`.

## Testing a change

`make test CRATES=app-sandbox-cli` runs the command against the pinned tools. A change to the
skill or the command also needs the skill followed on a real mod, such as
enough-internet-for-today's `digest` (`--props '{"demo":true}'`, commands in its README), from
start to stop, recording included: drive a command, read its `<mod>.debug`, take a shot, Read
it. Beside it, sample the user's windows and focus as [The app sandbox](app-sandbox.md) shows:
no window appears, and the focus moves only when the user moves it.
