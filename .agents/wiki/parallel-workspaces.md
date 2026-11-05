# Parallel workspaces

How to run the dev shell and the gate in a second jj workspace, and clean up.

A jj workspace has no `.git`, so `nix develop` there copies the whole directory, `target/`
included, into the Nix store. Enter the dev shell through `scripts/dev-shell`, which hands Nix a
copy of `flake.nix` and `flake.lock` alone, a changed flake included:

```sh
scripts/dev-shell scripts/check check
```

`.envrc` enters the same copy for direnv (`use flake "path:$(scripts/dev-shell --flake)"`).
After an edit of `.envrc`, direnv blocks it until `direnv allow`.

`scripts/dev-shell` never writes `flake.lock` back: after a change to the inputs of
`flake.nix`, every run works from a lock Nix updates in memory, until you run `nix flake lock`
in the checkout.

jj records a workspace's files only when a jj command runs there: run `jj st` after each batch
of edits, so the user sees the work from the main checkout with `jj log`.

Once a workspace's change has landed or been dropped, forget it and delete its folder: `jj
workspace forget <name>`, then `rm -r` with the folder's literal path.
