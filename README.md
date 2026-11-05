# herdr-common

Shared pieces for the apps that run on Herdr: visual testing first (a vision MCP core, a
headless sandbox, a fake agent, the `herdr-mod-vision` skill), pinned Herdr tools in one flake.

## Using it from an app

Check herdr-common out beside the app, then take its Rust crates through cargo `path`
dependencies:

```toml
[dev-dependencies]
<crate> = { path = "../herdr-common/crates/<crate>" }
```

No crate is published, and none is pinned by git revision: the app builds against the
herdr-common next to it.

herdr-common is test tooling only. Someone who clones an app alone has no `../herdr-common`,
so an app takes these crates as dev-dependencies, or from a test workspace of its own, never in
what it builds and installs. Code an app itself links lives in a small crate of its own, which
the app takes as a `git` dependency.

## Crates

- `app-sandbox`: a private herdr-app on a headless Weston display, in bubblewrap, for tests
  that drive and screenshot the app without touching your display or focus.
- `app-sandbox-cli`: the `app-sandbox` command, which keeps an app sandbox running across
  shell commands, for agents.

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
