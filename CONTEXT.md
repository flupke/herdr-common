# herdr-common

What the apps on Herdr share for visual testing, and the Herdr tools they pin, in one place.

## Language

**App**:
A program built on Herdr that uses herdr-common for its tests: herdr-bowtie,
herdr-progressive-reviewer, herdr-ruminate, enough-internet-for-today. Each lives in its own
repository, which someone may clone alone, save a private app such as herdr-bowtie, always
beside herdr-common.
_Avoid_: client, consumer, project

**Test tooling**:
What an app's tests and its agents' visual testing run, and never the app itself: all of
herdr-common but `vision-signal`, which apps link (ADR 0003). An app takes it as a `git`
dev-dependency, or by `path` from a test workspace of its own; a private app always beside
herdr-common, as herdr-bowtie, may take it by plain `path` (ADR 0004).
_Avoid_: shared code, common library

**Pinned tool**:
One of the releases of Herdr (`herdr`), herdr-app or herdr-core (`hc`) that `flake.nix` fixes,
which tests run whatever the user has installed. The dev shell names each in a `TEST_*_BIN_PATH`
variable, never on `PATH`.
_Avoid_: test binary, vendored Herdr

**The user's Herdr**:
The Herdr, herdr-app and hc installed on the machine, on `PATH`, which the user works in. Tests
never touch them.

**Fake agent**:
The program that plays Claude Code, or Codex, in a pane of a private Herdr or herdr-app for an
app's tests: it runs the app's hooks, shows its state as the real agent does, and reports what it
does to the test, which drives it with commands. Each app runs it from a program of its own.
_Avoid_: stand-in, stand-in agent, mock agent

**Visual testing**:
An agent running an app's Herdr screen in a private Herdr or herdr-app, seeing it through
screenshots and acting on it through commands, as the app's user would.
_Avoid_: e2e (an e2e test may check without seeing)

**App sandbox**:
A private herdr-app on a headless display (`crates/app-sandbox`): the pinned herdr-app and hc,
with their own `HERDR_APP_HOME`, `HC_SESSION`, XDG directories and Weston display, which never
shows on the user's display nor takes their focus. In bubblewrap by default, or plain where
bubblewrap cannot run.
_Avoid_: private app, test app

**Restart**:
What `AppSandbox::restart` does to the app sandbox's herdr-app, as a reboot or a crash would:
`core` kills the core and every program in its terminals, and the window starts a new core;
`reboot` kills the window and the core and starts the core alone; `window` opens the window
again after a reboot. A kept terminal comes back as a shell, at its address.
_Avoid_: relaunch, respawn

**Holder**:
The process that `app-sandbox start` leaves running, which holds an app sandbox and answers the
commands that follow on a socket named after the sandbox, until `stop`: an agent drives the
sandbox with one shell command after another.
_Avoid_: daemon, server

**Vision core**:
The vision MCP tools every app's visual testing shares (`crates/vision-mcp`): `screen`,
`screenshot`, `key`, `type`, `click`, `drag`, `settle` and `wait_for`, on a vision session over
an app sandbox. An app's own MCP server routes them beside its app-specific tools.
_Avoid_: vision server (each app has its own), driver

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
