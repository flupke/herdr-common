---
name: pin-herdr-tools
description: "Change herdr-common's pinned release of Herdr, herdr-app or hc (herdr-core) in flake.nix, and check any change to flake.nix or flake.lock. Use when moving a Herdr tool to another release, or when a change touches the flake."
---

`flake.nix` pins the releases of Herdr, herdr-app and hc that every app's tests run, whatever
the user has installed: `herdrVersion`, `herdrAppVersion` and `hcVersion`. The dev shell names
them in `TEST_HERDR_BIN_PATH`, `TEST_HERDR_APP_BIN_PATH` and `TEST_HC_BIN_PATH`, never on
`PATH`; `packages.<system>.{herdr,herdr-app,hc}` give them to an app's flake; and
`checks.<system>.pinned-versions` fails unless each tool's `--version` prints its pin.

## 1. Pick the releases

- Herdr: `gh release list --repo herdrdev/herdr` shows the latest.
- herdr-app and hc: GitHub has their tags but no assets; releases.herdr.dev serves them. A
  herdr-app release takes hc from its manifest's `core_min` on (0.1.11 for herdr-app 0.1.13):
  keep `hcVersion` at that or later.
- Keep hc at 0.1.20 or later: 0.1.19 and earlier type a `terminal.send` its caller gave up on
  late, so a resend can double a prompt.

When a tool's docs leave a behaviour out, read its source at the pinned release:
`git clone --depth 1 --branch v<version> https://github.com/herdrdev/<herdr|herdr-app|herdr-core>`.

## 2. Set the pins

- Herdr: set `herdrVersion` and the `hash` of each `herdrAssets` entry. GitHub lists each
  asset's SHA-256:

  ```sh
  gh release view v<version> --repo herdrdev/herdr --json assets \
    --jq '.assets[] | "\(.name) \(.digest)"'
  nix hash convert --hash-algo sha256 --to sri <hex>   # for each asset
  ```

- herdr-app and hc: set the version and the `sha256` of each Linux target, in hex, as the
  release's manifest lists them:

  ```sh
  curl -fsSL https://releases.herdr.dev/app/<version>/manifest.json
  curl -fsSL https://releases.herdr.dev/core/<version>/manifest.json
  ```

  The app's ELF needs `autoPatchelfHook`, and it opens Vulkan, EGL and Wayland at run time, so
  a library a new release opens goes in `runtimeDependencies`.

Change the versions in `README.md` ("The pinned Herdr tools") with them.

## 3. Check the flake

Run these from the checkout, in a jj workspace too. A jj workspace has no `.git`, so a plain
`nix flake check .` copies the whole folder, `target/` included, into the store:
`scripts/dev-shell --flake` hands Nix a copy of `flake.nix` and `flake.lock` alone.

```sh
nix flake check --no-write-lock-file "path:$(scripts/dev-shell --flake)" -L
nix build --no-link --rebuild -L \
  "path:$(scripts/dev-shell --flake)#checks.x86_64-linux.pinned-versions"
scripts/dev-shell sh -c '"$TEST_HERDR_BIN_PATH" --version; "$TEST_HERDR_APP_BIN_PATH" --version;
  "$TEST_HC_BIN_PATH" --version; command -v herdr herdr-app hc'
```

- The first prints `all checks passed!`. It prints each tool's `--version` only when it builds
  the check anew, as after a pin change.
- The second runs the check again whatever is cached.
- The third prints the pinned versions, then the paths of the user's own tools, outside
  `/nix/store`: the pins shadow nothing on `PATH`.

`nix flake check` checks this machine's system only. `--all-systems` evaluates the others, and
fails on any system the locked nixpkgs dropped, so the flake lists only the systems it supports.

`scripts/dev-shell` never writes `flake.lock` back: after a change to the flake's inputs, run
`nix flake lock` in the checkout.

The step is done when the three commands print what they should, and the gate
(`scripts/dev-shell scripts/check check`) passes on the new pins.
