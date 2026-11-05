# Apps take test tooling by path, and link nothing from here

herdr-bowtie, herdr-progressive-reviewer and enough-internet-for-today each built their own
visual-testing tools; herdr-common gathers them, and the apps need a way to use its crates.

An app takes a crate through a cargo `path` dependency, `../herdr-common/crates/<name>`, with
herdr-common checked out beside it. No crate is published, and no app pins a git revision: a
change here reaches every app at its next build, and one person works on both sides at once
without a release step.

A path resolves only where herdr-common sits beside the app, so someone who clones an app
alone cannot build what depends on it. So herdr-common holds test tooling only: an app takes it
as a dev-dependency, or from a test workspace of its own, never in what it builds and installs.
Code an app itself links stays out of herdr-common, in a tiny crate of its own that the app
takes as a `git` dependency.

Considered: publishing the crates on crates.io, or git dependencies pinned by revision. Either
lets an app build alone, but each change here would need a release or a pin bump in every app
before it could be tried there, for tools that only the maintainer's checkouts run.
