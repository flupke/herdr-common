# herdr-common

Shared pieces for the apps that run on Herdr: visual testing first (a vision MCP core, a
headless sandbox, a fake agent, the `herdr-mod-vision` skill), pinned Herdr tools in one flake.

## Using it from an app

No crate is published. An app takes the crates it tests with as `git` dev-dependencies, its
`Cargo.lock` pinning the herdr-common revision (`cargo update -p <crate>` moves it):

```toml
[dev-dependencies]
<crate> = { git = "https://github.com/flupke/herdr-common" }
```

Or it takes them by `path`, but only from a test workspace of its own, with its own
`[workspace]` and excluded from the app's, and herdr-common checked out beside the app:

```toml
# in <app>/tests/tui/Cargo.toml
[dependencies]
<crate> = { path = "../../../herdr-common/crates/<crate>" }
```

Not by `path` from the app's own workspace: cargo reads every path dependency of a workspace,
dev-dependencies included, so someone who clones the app alone could not build it. A private
app that always sits beside herdr-common, as herdr-bowtie, may still take plain `path`
dependencies.

herdr-common is test tooling, but for one crate: an app never takes these crates in what it
builds and installs. The one crate an app links, `vision-signal`, has no dependency, and the app
takes it from git:

```toml
[dependencies]
vision-signal = { git = "https://github.com/flupke/herdr-common" }
```

## Crates

- `app-sandbox`: a private herdr-app on a headless Weston display, in bubblewrap, for tests
  that drive and screenshot the app without touching your display or focus. It restarts the
  app's core, its window, or both, as a reboot would.
- `app-sandbox-cli`: the `app-sandbox` command, which keeps an app sandbox running across
  shell commands, for agents.
- `vision-mcp`: the vision MCP core, an agent's eyes and hands on an app in the app sandbox:
  `screen`, `screenshot`, `key`, `type`, `click`, `drag`, `settle` and `wait_for`, which an
  app's own MCP server routes beside its own tools. `settle` waits for the app to show a change an
  app's own tools made, and screens name the rows that changed since the previous one. A session sees and drives the app again after each restart.
- `vision-display`: the sandbox's X display driven as its user would, through XTEST and the
  clipboard: the core's hands.
- `vision-signal`: the frame markers and acknowledgements an app writes to its terminal, so the
  core returns once the app painted its reaction to an input.
- `fake-agent`: a fake Claude Code or Codex in a pane, which a test drives.

## The herdr-mod-vision skill

`skills/herdr-mod-vision` lets a coding agent see and drive the herdr-app mod it builds, in an
app sandbox: commands, the mod's state, screenshots and recordings, without taking your window.
Install it as a link, so it runs this checkout's `app-sandbox`, once any older copy of the
skill is out of the way:

```sh
ln -sfn "$PWD/skills/herdr-mod-vision" ~/.claude/skills/herdr-mod-vision
```

## The pinned Herdr tools

`flake.nix` pins Herdr 0.9.5, herdr-app 0.1.13 and hc 0.1.18 (herdr-app and hc on Linux only).
`nix develop` names them in `TEST_HERDR_BIN_PATH`, `TEST_HERDR_APP_BIN_PATH` and
`TEST_HC_BIN_PATH`, and leaves them off `PATH`, so `herdr`, `herdr-app` and `hc` there stay
your own. The flake's `packages` (`herdr`, `herdr-app`, `hc`) give the same releases to another
flake.

## License

MIT: see [LICENSE](LICENSE).
