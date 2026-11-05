# Apps take test tooling by git, or from a test workspace

Supersedes ADR 0001 where it says that apps take test tooling as `path` dependencies, and ADR
0003 where it repeats it ("every other crate here is test tooling an app takes by path").

ADR 0001 held that a `path` dev-dependency on `../herdr-common` spares someone who clones an
app alone. It does not: cargo reads every path dependency of a workspace, dev-dependencies
included, to resolve it, so where `../herdr-common` is missing `cargo build`, `cargo build -p`
and `cargo install --path` all fail ("failed to read ../herdr-common/crates/<name>/Cargo.toml").
herdr-ruminate's migration hit it, and herdr-bowtie's confirmed it on cargo 1.90. A separate test
workspace holds: the app's own workspace never reads its manifest.

An app takes a herdr-common crate in one of two ways:

- by `path`, only from a test workspace of its own, a Cargo workspace apart from the app's (its
  own `[workspace]`, excluded from the app's), as herdr-ruminate's `tests/tui`;
- as a `git` dependency on this repository when the app's own workspace needs the crate,
  as a dev-dependency for test tooling. The app's `Cargo.lock` then pins a herdr-common
  revision, which `cargo update -p <crate>` moves.

An app that is private and always sits beside herdr-common, as herdr-bowtie, may still take
plain `path` dependencies: nobody clones it alone.

The rest of ADR 0001 holds: herdr-common is test tooling, which no app links into what it
builds and installs, save `vision-signal` (ADR 0003), and no crate is published.

Considered: keeping `path` dev-dependencies in the app's own workspace, which breaks a lone
clone as above; and a test workspace for every app, which makes an app whose tests already
live in its own workspace move them out; and `git` dependencies for herdr-bowtie too, which
would make each change here a lock update there for no clone that needs it. The git dependency costs what ADR 0001 avoided: a
change here reaches that app only once it updates its lock file.
