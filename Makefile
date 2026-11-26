.PHONY: check complexity fmt lint test

# These targets build with the same flags, so they share one build cache.
lint test check: export RUSTFLAGS = -Dwarnings

# typos skips hidden folders unless named: name a new hidden folder of text here. A right word
# it flags goes in _typos.toml, under [default.extend-words].
lint: complexity
	cargo clippy --workspace --all-targets
	typos . .agents

# nextest runs no doc test, so a run without CRATES runs them with cargo test, and nextest fails
# a run that finds no test unless --no-tests=pass.
# A pane of the user's Herdr sets the HERDR_* variables, one of the user's herdr-app the HC_*
# ones: the tests run without them, so none reaches the user's Herdr or herdr-app.
CRATES ?=
NO_LIVE_HERDR = env -u HERDR_ENV -u HERDR_SOCKET_PATH -u HERDR_PANE_ID -u HERDR_WORKSPACE_ID \
	-u HERDR_TAB_ID -u HC_SCOPE -u HC_SESSION -u HC_SOCKET -u HC_TERMINAL_ID \
	-u HC_TERMINAL_SOCKET
test:
ifeq ($(strip $(CRATES)),)
	$(NO_LIVE_HERDR) cargo test --workspace --doc
	$(NO_LIVE_HERDR) cargo nextest run --workspace --no-tests=pass
else
	$(NO_LIVE_HERDR) cargo nextest run --no-tests=pass $(foreach crate,$(CRATES),-p $(crate))
endif

fmt:
	cargo fmt --all

check: lint
	$(MAKE) test CRATES=

complexity:
	@report="$$(cccc --lang rust crates | jq -r '[.files[] | .path as $$path | .functions[] | recurse(.children[]?) | select(.cyclomatic > 10 or .cognitive > 15) | { path: $$path, line, name, cognitive, cyclomatic }] | sort_by([-.cyclomatic, -.cognitive, .path, .line]) | if length == 0 then empty else ("Cognitive\tCyclomatic\tFunction", (.[] | "\(.cognitive)\t\(.cyclomatic)\t\(.path):\(.line) \(.name)")) end')" || exit; \
	if [ -n "$$report" ]; then printf '%s\n' "$$report"; exit 1; fi
