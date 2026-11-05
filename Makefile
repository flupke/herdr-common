.PHONY: check complexity fmt lint test

# The checks come in levels, cheapest first; AGENTS.md says when to run which.
# They all build with the same flags, so they share one build cache.
lint test check: export RUSTFLAGS = -Dwarnings

# Cargo refuses a workspace without a member, so the cargo steps wait for the first crate in
# crates/ (.agents/wiki/checks.md).
CARGO_MEMBERS := $(wildcard crates/*/Cargo.toml)

# Level 1: lints of every target, the complexity gate, and spelling (typos, which skips hidden
# folders unless named: a right word it flags goes in _typos.toml).
lint: complexity
ifneq ($(CARGO_MEMBERS),)
	cargo clippy --workspace --all-targets
endif
	typos . .agents

# Level 2, with CRATES: the unit tests of those crates (`make test CRATES="sandbox"`).
# Level 3, without: every unit test of the workspace, doc tests included.
# A pane of the user's Herdr sets the HERDR_* variables, one of the user's herdr-app the HC_*
# ones: the tests run without them, so none reaches the user's Herdr or herdr-app.
CRATES ?=
NO_LIVE_HERDR = env -u HERDR_ENV -u HERDR_SOCKET_PATH -u HERDR_PANE_ID -u HERDR_WORKSPACE_ID \
	-u HERDR_TAB_ID -u HC_SCOPE -u HC_SESSION -u HC_SOCKET -u HC_TERMINAL_ID \
	-u HC_TERMINAL_SOCKET
test:
ifneq ($(CARGO_MEMBERS),)
ifeq ($(strip $(CRATES)),)
	$(NO_LIVE_HERDR) cargo test --workspace --doc
	$(NO_LIVE_HERDR) cargo nextest run --workspace --no-tests=pass
else
	$(NO_LIVE_HERDR) cargo nextest run --no-tests=pass $(foreach crate,$(CRATES),-p $(crate))
endif
endif

# Formats the code, once a change is complete. No check fails on formatting.
fmt:
ifneq ($(CARGO_MEMBERS),)
	cargo fmt --all
endif

# Every level in one run: the gate of a change (AGENTS.md, step 5).
check: lint
	$(MAKE) test CRATES=

complexity:
ifneq ($(CARGO_MEMBERS),)
	@report="$$(cccc --lang rust crates | jq -r '[.files[] | .path as $$path | .functions[] | recurse(.children[]?) | select(.cyclomatic > 10 or .cognitive > 15) | { path: $$path, line, name, cognitive, cyclomatic }] | sort_by([-.cyclomatic, -.cognitive, .path, .line]) | if length == 0 then empty else ("Cognitive\tCyclomatic\tFunction", (.[] | "\(.cognitive)\t\(.cyclomatic)\t\(.path):\(.line) \(.name)")) end')" || exit; \
	if [ -n "$$report" ]; then printf '%s\n' "$$report"; exit 1; fi
endif
