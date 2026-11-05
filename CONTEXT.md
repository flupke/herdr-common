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

**Visual testing**:
An agent running an app's Herdr screen in a private Herdr or herdr-app, seeing it through
screenshots and acting on it through commands, as the app's user would.
_Avoid_: e2e (an e2e test may check without seeing)
