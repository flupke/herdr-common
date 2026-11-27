---
name: test-sandbox-change
description: "What Herdr, herdr-app and hc do in a private Herdr or app sandbox, and how to run a change to herdr-common's app sandbox, vision core, fake agent, herdr-mod-vision skill or app-sandbox command as its user would. Use before changing or testing any of them, or when a behaviour of Herdr, herdr-app or hc is in question."
---

Each part below names its unit tests, then the run that the unit tests cannot stand for. Run
everything in the dev shell (`scripts/dev-shell ...`), against the pinned tools, never the
user's Herdr, herdr-app or panes. Stop only the processes you started, by their PID.

What Herdr, herdr-app and hc do in a private one, which the code leans on:

- [references/herdr-app-and-hc.md](references/herdr-app-and-hc.md): herdr-app and hc in an app
  sandbox: restarts, typing into a kept terminal, PIDs, `shot`, the window, input, what
  `hc events`, `hc read` and `hc keys` give, recording.
- [references/herdr.md](references/herdr.md): what a Herdr server reads of a fake agent.

Read the one for the tool your change leans on before you change the code or design the run.

## The app sandbox (`crates/app-sandbox`)

`make test CRATES=app-sandbox` runs the sandbox in each isolation, in bubblewrap on the machine's
GPU when it has one and in plain isolation in software; `make test CRATES=vision-mcp`
restarts its core, reboots it and opens the window again, and drives a probe after each.

A change to what keeps the sandbox off the user's display also needs the **window watch**: list
the windows of the user's desktop and which one has the focus, every half second while the
tests run, and check the list never changes. On sway:

```sh
out="${TMPDIR:-/tmp}/windows.json"
snap() { swaymsg -t get_tree | jq -c '[recurse(.nodes[]?, .floating_nodes[]?)
  | select(.pid != null) | {id, name, focused}]'; }
(while true; do snap; sleep 0.5; done) >"$out" & sampler=$!
scripts/dev-shell scripts/check test CRATES=app-sandbox
kill "$sampler"; sort -u "$out" | wc -l   # 1: no window appeared, the focus never moved
```

For another run, put its commands in place of the `scripts/check` line. On X11,
`xdotool getwindowfocus` and `wmctrl -l` give the same.

A test that breaks the sandbox on purpose leaves its processes behind in plain isolation: find
them by the mark, `APP_SANDBOX=<directory>` in `/proc/<PID>/environ`, and kill those alone. A
process that both cleared its environment and left its parents (`setsid env -i`) has no mark
and outlives a plain sandbox; in bubblewrap the kernel ends it.

## The vision core (`crates/vision-mcp`, `vision-display`, `vision-signal`)

`make test CRATES=vision-mcp` drives `vision-probe` and a shell in the sandbox through the MCP
tools. A change to the display input also needs the window watch above, with
`CRATES=vision-mcp`.

## The herdr-mod-vision skill and the `app-sandbox` command

`make test CRATES=app-sandbox-cli` runs the command. A change to the skill or the command also
needs the skill followed on a real mod, from start to stop, recording included, such as
enough-internet-for-today's `digest` (`--props '{"demo":true}'`, its commands in its README):
drive a command, read its `<mod>.debug`, take a shot, Read it, record a video, Read a frame. Run
the window watch beside it: no window appears, and the focus moves only when the user moves it.
A gotcha the run meets that the skill lacks goes in `skills/herdr-mod-vision/SKILL.md`.

The skill's `app-sandbox` script builds the checkout that holds it: run the one of the checkout
you changed, as `<checkout>/skills/herdr-mod-vision/app-sandbox`.

## The fake agent (`crates/fake-agent`)

The checks run neither Herdr nor herdr-app: a change to what either reads of the agent also
needs a run in a private one.

### In a private herdr-app

With the `herdr-mod-vision` skill's `app-sandbox` script as `$S`. The agent runs in the
sandbox's core, the listener outside it, on a socket under `target/`, which the sandbox sees.

```sh
cargo build -p fake-agent --examples --bins
V=$PWD/target/vision-app; mkdir -p $V/bin; ln -sf $PWD/target/debug/fake-agent $V/bin/claude
mkfifo $V/commands
$S start <name>
setsid sh -c "tail -f $V/commands | target/debug/examples/listen $V/events.sock t1 \
  >$V/events.log 2>&1" &
$S hc <name> run --bg --keep --name agent -- env FAKE_AGENT_EVENTS=$V/events.sock \
  FAKE_AGENT_NAME=t1 $V/bin/claude
echo '{"command":"show_working","working":true}' >$V/commands
$S hc <name> get agent --json   # its status
```

Once done, `$S stop <name>`, then stop the listener and its `tail` by their PIDs.

### In a private Herdr server

A Unix socket path has a short limit: keep the server's under `target/`, not the scratchpad.
Point every Herdr and XDG variable inside:

```sh
cargo build -p fake-agent --examples --bins
V=$PWD/target/vision; mkdir -p $V/{home,config,state,data,cache,runtime,bin}; touch $V/config.toml
export HOME=$V/home XDG_CONFIG_HOME=$V/config XDG_STATE_HOME=$V/state XDG_DATA_HOME=$V/data \
  XDG_CACHE_HOME=$V/cache XDG_RUNTIME_DIR=$V/runtime HERDR_SOCKET_PATH=$V/herdr.sock \
  HERDR_CONFIG_PATH=$V/config.toml
unset HERDR_ENV HERDR_PANE_ID HERDR_WORKSPACE_ID HERDR_TAB_ID HC_SCOPE HC_SESSION HC_SOCKET \
  HC_TERMINAL_ID HC_TERMINAL_SOCKET CLAUDE_CONFIG_DIR
H=$TEST_HERDR_BIN_PATH
ln -sf $PWD/target/debug/fake-agent $V/bin/claude
setsid $H server >$V/server.log 2>&1 &         # stop: $H server stop
$H workspace create --no-focus                  # its pane: w1:p1
mkfifo $V/commands
setsid sh -c "tail -f $V/commands | target/debug/examples/listen $V/events.sock w1:p1 \
  >$V/events.log 2>&1" &                        # a command: echo '{"command":"end_turn"}' >$V/commands
$H pane run w1:p1 "FAKE_AGENT_EVENTS=$V/events.sock FAKE_AGENT_HERDR=$H $V/bin/claude \
  -- 'A first prompt.'"
$H pane get w1:p1     # agent and agent_status
$H pane read w1:p1    # its screen
```

Name the agent by its full path in `pane run`: the user's `PATH` may hold a space, which an
`env PATH=…` there splits. Once done, stop the server, then the listener by its PID.

The run is done when each behaviour the change touched has been seen in the private Herdr or
herdr-app, and every process you started is stopped.
