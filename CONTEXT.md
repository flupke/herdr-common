# herdr-common

What the apps on Herdr share for visual testing, and the Herdr tools they pin, in one place.
Apps use these terms as written here. An _Avoid_ line names words not to use for the term: its
synonyms, and words that name another concept.

## Language

### Apps and tools

**App**:
A program built on Herdr that uses herdr-common for its tests: herdr-bowtie,
herdr-progressive-reviewer, herdr-ruminate, enough-internet-for-today. Each lives in its own
repository, which someone may clone alone, save herdr-bowtie, a private repository always beside
herdr-common.
_Avoid_: client, consumer, project

**Test tooling**:
What an app's tests and its agents' visual testing run, and never the app itself: all of
herdr-common but `vision-signal`, which apps link (ADR 0003). An app takes it as a `git`
dev-dependency, or by `path` from a test workspace of its own; herdr-bowtie, always beside
herdr-common, may take it by plain `path` (ADR 0004).
_Avoid_: shared code, common library

**Pinned tool**:
One of the releases of Herdr (`herdr`), herdr-app or herdr-core (`hc`) that `flake.nix` fixes,
which tests run whatever the user has installed. The dev shell names each in a `TEST_*_BIN_PATH`
variable, never on `PATH`.
_Avoid_: test binary, vendored Herdr

**The user's Herdr**:
The Herdr, herdr-app and hc installed on the machine, on `PATH`, which the user works in. Tests
never touch them.

**Core**:
herdr-core's server (`hc`), which runs a herdr-app's terminals: the window connects to it, and
it runs without one.
_Avoid_: core alone for the vision core

**Private Herdr**:
A Herdr server, core or herdr-app of the pinned tools that a test or an agent runs for an app
or a mod, with its own socket, config and state, which nothing of the user's reaches. The
private herdr-app is an app sandbox; a private Herdr server runs bare, or in a bubble of its
caller's.
_Avoid_: test Herdr; sandbox alone, which is an app sandbox

**Fake agent**:
The program that plays Claude Code, or Codex, in a pane of a private Herdr for an app's tests: it
runs the app's hooks, shows its state as the real agent does, and reports what it does to the
test, which drives it with commands. Each app runs it from a program of its own, which is the
app's fake agent too.
_Avoid_: stand-in, stand-in agent, fake claude, mock agent

### Visual testing

**Visual testing**:
An agent running an app's Herdr screen in a private Herdr, seeing it through screenshots and
acting on it through commands, as the app's user would.
_Avoid_: e2e (an e2e test may check without seeing), UI exploration (Explore is
herdr-progressive-reviewer's interview)

**Vision server**:
An app's own MCP server for visual testing: its `start` opens a vision session on the app, and,
on an app sandbox, it routes the vision core's tools beside the app's own.
_Avoid_: vision core (the tools vision servers share)

**Vision session**:
A private Herdr that a vision server's `start` opens and its `stop` ends, on which its tools act.
The vision core's is an app sandbox, seen through its terminals' screens and its window's
pixels, and driven through its display.

**Vision core**:
The vision MCP tools every app's visual testing shares (`crates/vision-mcp`): `screen`,
`screenshot`, `key`, `type`, `click`, `drag`, `settle` and `wait_for`, on a vision session over
an app sandbox. An app's vision server routes them beside its app-specific tools.
_Avoid_: vision server (each app has its own), driver

**Frame signal**:
What an app under visual testing tells the vision core through its terminal's bytes: a frame
marker after each frame, and acknowledgements (`crates/vision-signal`, the one crate here that
apps link). An app that sends it speaks vision-signal.

**Frame marker**:
The terminal title an app that speaks vision-signal writes after each frame it paints: the
frame's number, the latest acknowledgement it read, and its size. herdr-core reports it as a
`title` event, after the frame's bytes.

**Acknowledgement**:
How an app that speaks vision-signal confirms it read an input: after the input, or at once
for a settle, the vision core pastes an acknowledgement request, and the first frame marker that
names it shows the app's reaction to what reached its terminal before.

**Settle**:
What `Vision::settle` and the `settle` tool do: wait for the app to show a change no input of
the vision core made, such as one an app's own tools make. With an acknowledgement when the
focused terminal's program speaks vision-signal, which shows what reached its terminal alone,
else once the window's pixels changed and stayed quiet, as after an input (`settled` in
`Reaction::Pixels`). On pixels alone, whatever has the focus, with `settle_pixels` (the tool's
`pixels`).

**Changed rows**:
The rows of a terminal's screen whose text differs from the previous screen of that terminal
the vision core returned, by any tool; none on the first.

### The app sandbox

**App sandbox**:
A private herdr-app on a headless display (`crates/app-sandbox`): the pinned herdr-app and hc,
with their own `HERDR_APP_HOME`, `HC_SESSION`, XDG directories and Weston display, which never
shows on the user's display nor takes their focus. In bubblewrap by default, or plain where
bubblewrap cannot run.
_Avoid_: private app, test app

**Bubble**:
The bubblewrap environment an app sandbox runs in by default (`app_sandbox::Bubble`): PID and
mount namespaces of its own, a tmpfs on `/tmp`, the directories its caller masks shown empty,
such as the user's (`user_directories`), and the host's network or a loopback alone. A caller
may run one alone, with no herdr-app, such as herdr-bowtie for a Herdr server. Its init is its
first process: once the init ends, everything in the bubble ends.
_Avoid_: container; holder, for its init

**Forward**:
A TCP port of the app sandbox's loopback that the host reaches at the same `127.0.0.1:<port>`
(`AppSandbox::forward`), such as a server's that an app runs in the sandbox and its test calls.
_Avoid_: tunnel, port mapping

**Restart**:
What `AppSandbox::restart` does to the app sandbox's herdr-app, as a reboot or a crash would:
`core` kills the core and every program in its terminals, and the window starts a new core;
`reboot` kills the window and the core and starts the core alone; `window` opens the window
again after a reboot. A kept terminal comes back as a shell, at its address.
_Avoid_: relaunch (herdr-bowtie's resume of an agent's session), respawn

**Holder**:
The process that `app-sandbox start` leaves running, which holds an app sandbox and answers the
commands that follow on a socket named after the sandbox, until `stop`: an agent drives the
sandbox with one shell command after another.
_Avoid_: daemon, server; holder alone for herdr-core's holders (which keep its terminals) or a
bubble's init
