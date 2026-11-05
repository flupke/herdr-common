# Tests

Which tests to write and keep, and the tautological tests code review removes.

## Policy

Test the behaviour that matters once, at the lowest level that sees it. A crate here serves
the apps' tests, so its own tests check what an app relies on: that a sandbox starts the pinned
tool and stops all it started, that a screenshot shows what the screen holds. No tests for
crash, restart or race recovery unless the user asks. Every change deletes the tests it makes
obsolete or redundant, and a smaller suite counts as progress.

A test earns its place when it checks behaviour a caller relies on, that the code does not
make obvious, at the lowest level that sees it, and that no other test checks. Judge it by two
edits: break the behaviour (a wrong value, an effect skipped or done twice) and it fails;
change only the internals, with every observable result the same, and it still passes. A test
that survives the first edit is tautological; one that fails the second is pinned to the
implementation: rewrite it to assert what callers observe, or delete it.

## Tests that can go red

- The expected value is written by hand from the behaviour the code owes, or comes from an
  independent source: an external tool's output, a second code path.
- The assertion reads what production code produced: a return value, a file, a rendered frame.
- A format this repository owns is compared with a hand-written literal, so a rename breaks the
  test as it would break the apps that read it.

## Tautological tests

Code review deletes a tautological test, rewrites it to assert what the code computes, or, for
a harness, marks it ignored. The kinds:

- **Mirror**: the expected value is computed by the code under test, or by a copy of its logic,
  so a bug lands on both sides.
- **Echo**: the test reads back a field it set, a getter of a constructor argument, or a clone.
- **Constant pin**: `assert_eq!(LIMIT, 16_777_216)`. Build the boundary input from the constant
  instead.
- **Library behaviour**: std, serde derives without custom attributes, a dependency's own
  behaviour. The dependency's own tests cover them.
- **Fake echo**: every value asserted comes from a fake the test configured, through code that
  only passes it on.
- **Vacuous assertion**: true for every value the type can hold, or met by an error reply as
  well as a success.
- **Harness counted as a test**: a `#[test]` that a parent test runs as a child process and
  that returns early unless an environment variable is set. Mark it `#[ignore = "<what runs
  it>"]` and have the parent pass `--ignored`.

## Waiting

A test never waits on the wall clock: no sleep, no loop that polls until a deadline, no window
that proves nothing happened. Give the code under test its durations or its clock as values
the test sets, and wake the test with an event: a channel, an event the code or the tool
already publishes, such as Herdr's event stream or `hc events`. Prove that nothing happened by
ordering: send a marker through the same path and wait for it. A test may block on an event
without a timeout of its own: nextest ends one still running after two minutes
(`.config/nextest.toml`) and reports it as failed.

A screen sends no event: a terminal emulated for a Herdr client, and a private herdr-app's
window. A test waits for a text to show there, or for the window's pixels to change and
settle; those are the only polls a test may run.
