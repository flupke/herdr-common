# The frame signal apps link is a crate here, taken by git

Supersedes ADR 0001 where it says that code an app links stays out of herdr-common.

The vision core (`crates/vision-mcp`) learns when an app reacted from the frame markers and
acknowledgements the app writes to its terminal: the reviewer's vision-signal protocol. The
app links the code that writes them into what it builds and installs; the core, test tooling,
parses them. Both sides must share one format, or they drift as the apps' two vision tools did.

`crates/vision-signal` holds the protocol, here, with no dependency. The core takes it by path,
as all test tooling here. An app takes it as a `git` dependency on this repository
(`vision-signal = { git = "https://github.com/flupke/herdr-common" }`), so that someone who
clones the app alone still builds it. The rest of ADR 0001 holds: every other crate here is test
tooling an app takes by path, and no other crate here is linked by an app.

Considered: a repository of its own for the crate, which keeps herdr-common free of anything an
app links, at the cost of a second repository for a hundred lines that change with the core;
and leaving it in herdr-progressive-reviewer, which would make the shared core depend on one of
the apps it serves.
