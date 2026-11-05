# herdr-common

What the apps on Herdr share for visual testing, and the Herdr tools they pin, in one place.

## Language

**App**:
A program built on Herdr that uses herdr-common for its tests: herdr-bowtie,
herdr-progressive-reviewer, enough-internet-for-today. Each lives in its own repository,
checked out beside herdr-common.
_Avoid_: client, consumer, project

**Test tooling**:
What an app's tests and its agents' visual testing run, and never the app itself: what
herdr-common holds. An app takes it as a dev-dependency or from a test workspace of its own.
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
