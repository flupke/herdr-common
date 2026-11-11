# The fake agent

What Herdr and herdr-app read of `crates/fake-agent`, and how to watch it in a private Herdr
server or herdr-app. The crate's docs (`src/lib.rs`, `variables`, `Command`, `Event`) say what it
does and how an app runs it, `tests/agent.rs` how its tests stand for Herdr and herdr-app; ADR
0002 says why an app runs it from a program of its own.

The checks run neither Herdr nor herdr-app: a change to what either reads of the agent also
needs a run in a private one (below).

## What Herdr 0.9.5 reads of it

Checked against the pinned Herdr, with the fake agent in a private server's pane:

- Herdr detects the agent by its process's name: the pane runs the program through a link named
  `claude` (or `codex`), and Herdr gives the pane `agent: claude`.
- Its status comes from the title: `✳ ` first is `idle`, a braille spinner first is `working`.
- Claude Code's permission form after the last rule (`─…`) is `blocked`; the input line between
  rules that follows it is `idle` again.
- With `FAKE_AGENT_TURNS=auto`, a turn ends once Herdr's `pane.agent_status_changed` says
  `working`: the agent subscribes on `HERDR_SOCKET_PATH` before it shows anything, and asks
  `agent.get` for the status at that moment.
- Herdr's events name an event with a dot (`pane.agent_status_changed`) or an underscore
  (`pane_agent_detected`): read both.

## What herdr-app reads of it

Checked against the pinned herdr-app 0.1.13 and hc, in an app sandbox: with `HC_TERMINAL_ID`
set, the agent reports its state in OSC 7501, and `hc get <terminal> --json` gives it as
`status` (`{"app": "claude-code", "state": "working"}`).

## Watching it in a private herdr-app

In the dev shell, with the `herdr-mod-vision` skill's `app-sandbox` script as `$S`: the agent
runs in the sandbox's core, the listener outside it, on a socket under `target/`, which the
sandbox sees.

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

## Watching it in a private Herdr server

To see a change to what the agent shows, run it in a private server of the pinned Herdr, in the
dev shell, with a socket path short enough for a Unix socket (under `target/`, not the
scratchpad) and every Herdr and XDG variable pointed inside:

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
  >$V/events.log 2>&1" &                        # the events; a command: echo '{"command":"end_turn"}' >$V/commands
$H pane run w1:p1 "FAKE_AGENT_EVENTS=$V/events.sock FAKE_AGENT_HERDR=$H $V/bin/claude \
  -- 'A first prompt.'"
$H pane get w1:p1     # agent and agent_status
$H pane read w1:p1    # its screen
```

Name the agent by its full path in `pane run`: the user's `PATH` may hold a space, which an
`env PATH=…` there splits. Once done, stop the server, then the listener by its PID.
