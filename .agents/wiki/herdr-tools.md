# The pinned Herdr tools

The releases of Herdr, herdr-app and herdr-core (`hc`) that the tests run, whatever the user has
installed, and how to change them.

## What the flake gives

`flake.nix` pins them once: `herdrVersion`, `herdrAppVersion` and `hcVersion`. herdr-app and hc
are Linux only, since the sandbox that runs them is bubblewrap. The flake gives them three ways:

- The dev shell names them in `TEST_HERDR_BIN_PATH`, `TEST_HERDR_APP_BIN_PATH` and
  `TEST_HC_BIN_PATH`, never on `PATH`: `herdr`, `herdr-app` and `hc` there stay the user's.
  It also names a headless Weston (`TEST_WESTON_BIN_PATH`) and Mesa's software Vulkan driver
  (`TEST_HERDR_APP_VULKAN_DRIVER`) for a private herdr-app's display.
- `packages.<system>.herdr`, `herdr-app` and `hc`, for an app's flake that takes herdr-common's
  pins rather than its own.
- `checks.<system>.pinned-versions`, which `nix flake check` builds: it runs each tool's
  `--version` and fails unless it prints the pinned version.

Keep hc at 0.1.20 or later: it cancels a `terminal.send` its caller gave up on, where 0.1.19
and earlier type it late, so a resend can double a prompt.

herdr-app takes hc from its manifest's `core_min` on (0.1.11 for herdr-app 0.1.13). `gh
release list --repo herdrdev/herdr` shows Herdr's latest release.

## Checking a change to the flake

In a jj workspace, which has no `.git`, hand Nix the flake's copy as `scripts/dev-shell` does,
or it copies the whole folder, `target/` included:

```sh
nix flake check --no-write-lock-file "path:$(scripts/dev-shell --flake)" -L
nix build --no-link --rebuild -L \
  "path:$(scripts/dev-shell --flake)#checks.x86_64-linux.pinned-versions"
scripts/dev-shell sh -c '"$TEST_HERDR_APP_BIN_PATH" --version; "$TEST_HC_BIN_PATH" --version;
  command -v herdr herdr-app hc'
```

The first prints `all checks passed!`, and each pinned tool's `--version` only when it builds
the check anew, as after a pin change; the second runs the check again whatever is cached. The
third prints the pinned versions, then the paths of the user's own tools (not `/nix/store`):
proof the pins shadow nothing. `nix flake check` checks the machine's system only.
`--all-systems` evaluates the others, which fails on any system the locked nixpkgs dropped:
the flake lists only those it supports (it dropped `x86_64-darwin`).

## Changing a release

- Herdr: set `herdrVersion` and the `hash` of each `herdrAssets` entry. GitHub lists each
  asset's SHA-256: `gh release view v<version> --repo herdrdev/herdr --json assets --jq
  '.assets[] | "\(.name) \(.digest)"'`. Convert each one with `nix hash convert --hash-algo
  sha256 --to sri <hex>`.
- herdr-app and hc: set the version and the `sha256` of each Linux target, as the release's
  manifest lists them (hex, as Nix takes them):

  ```sh
  curl -fsSL https://releases.herdr.dev/app/<version>/manifest.json
  curl -fsSL https://releases.herdr.dev/core/<version>/manifest.json
  ```

  GitHub has tags but no release assets for them: releases.herdr.dev serves them. The app's ELF
  needs `autoPatchelfHook`; it opens Vulkan, EGL and Wayland at run time, so those go in
  `runtimeDependencies`.

Then run the commands of the section above, and the gate. Read a tool's source at the pinned
release when its docs leave a behaviour out: `git clone --depth 1 --branch v<version>
https://github.com/herdrdev/<herdr|herdr-app|herdr-core>`.
