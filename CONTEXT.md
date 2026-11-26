# herdr-common

The glossary of what the apps on Herdr share for visual testing, and the Herdr tools they pin.
Apps use these terms as written here. Each line says what the term is, then the words to avoid
for it: synonyms, and words that name another concept. Add or change a line only for a term the
user introduces or corrects.

## Apps and tools

- **App**: a program built on Herdr that uses herdr-common for its tests (herdr-bowtie, herdr-progressive-reviewer, herdr-ruminate, enough-internet-for-today), each in a repository someone may clone alone, save herdr-bowtie, which is private and always beside herdr-common. Avoid: client, consumer, project.
- **Test tooling**: what an app's tests and its agents' visual testing run, never the app itself: all of herdr-common but `vision-signal`, taken as a `git` dev-dependency or by `path` from a test workspace of the app's own, or by plain `path` in herdr-bowtie. Avoid: shared code, common library.
- **Pinned tool**: a release of Herdr (`herdr`), herdr-app or herdr-core (`hc`) that `flake.nix` fixes and the dev shell names in a `TEST_*_BIN_PATH` variable, never on `PATH`. Avoid: test binary, vendored Herdr.
- **The user's Herdr**: the Herdr, herdr-app and hc on the machine's `PATH`, which the user works in and tests never touch.
- **Core**: herdr-core's server (`hc`), which runs a herdr-app's terminals, with or without its window. Avoid: core alone for the vision core.
- **Private Herdr**: a Herdr server, core or herdr-app of the pinned tools, with its own socket, config and state, which a test or an agent runs and nothing of the user's reaches; the private herdr-app is an app sandbox. Avoid: test Herdr; sandbox alone.
- **Fake agent**: the program that plays Claude Code, or Codex, in a pane of a private Herdr for an app's tests, run from a program of the app's own and driven by the test's commands. Avoid: stand-in, stand-in agent, fake claude, mock agent.

## Visual testing

- **Visual testing**: an agent running an app's Herdr screen in a private Herdr, seeing it through screenshots and acting on it through commands, as the app's user would. Avoid: e2e, UI exploration.
- **Vision server**: an app's own MCP server for visual testing, whose `start` opens a vision session and which routes the vision core's tools beside the app's own. Avoid: vision core.
- **Vision session**: the private Herdr a vision server's `start` opens and its `stop` ends, on which its tools act.
- **Vision core**: the vision MCP tools every app's visual testing shares (`crates/vision-mcp`): `screen`, `screenshot`, `key`, `type`, `click`, `drag`, `settle` and `wait_for`. Avoid: vision server, driver.
- **Frame signal**: the frame markers and acknowledgements an app writes to its terminal for the vision core (`crates/vision-signal`, the one crate apps link); an app that sends it speaks vision-signal.
- **Frame marker**: the terminal title an app that speaks vision-signal writes after each frame: its number, the latest acknowledgement read, and the terminal's size.
- **Acknowledgement**: the app's confirmation, in a frame marker, of an acknowledgement request the vision core pasted after an input: that frame shows the app's reaction.
- **Settle**: the wait of `Vision::settle` and the `settle` tool for the app to show a change no input of the vision core made, by an acknowledgement or by the window's pixels.
- **Changed rows**: the rows of a terminal's screen whose text differs from the screen of that terminal the vision core returned before; none on the first.

## The app sandbox

- **App sandbox**: a private herdr-app on a headless Weston display (`crates/app-sandbox`), in bubblewrap or plain isolation, which never shows on the user's display nor takes their focus. Avoid: private app, test app.
- **Bubble**: the bubblewrap environment an app sandbox runs in (`app_sandbox::Bubble`), which a caller may also run alone; once its init ends, everything in it ends. Avoid: container; holder, for its init.
- **Forward**: a TCP port of the app sandbox's loopback that the host reaches at the same `127.0.0.1:<port>` (`AppSandbox::forward`). Avoid: tunnel, port mapping.
- **Restart**: what `AppSandbox::restart` does to the app sandbox's herdr-app, as a reboot or a crash would: `core`, `reboot` or `window`; a kept terminal comes back as a shell at its address. Avoid: relaunch, respawn.
- **Holder**: the process `app-sandbox start` leaves running, which holds an app sandbox and answers the commands that follow until `stop`. Avoid: daemon, server; holder alone for herdr-core's holders or a bubble's init.
