# Works the same from fish, bash or zsh:  make test
CARGO ?= cargo

.PHONY: build install test test-unit test-prop test-prop-long test-snapshots test-ui test-integration test-sync \
        update-snapshots docs report lint fmt clean

build:
	$(CARGO) build --release

install:
	$(CARGO) install --path . --locked
	@mkdir -p $(HOME)/.config/fish/completions
	cp completions/kuro.fish $(HOME)/.config/fish/completions/kuro.fish

# everything, in order; stops at the first failing group
test: test-unit test-prop test-snapshots test-ui test-sync test-integration

# pure logic, input parser, config, docs checks (config options and keybindings, both directions), recorded error fixtures
test-unit:
	$(CARGO) test --lib --test classify_fixtures --test docs_check --test keybindings --test cli --test syntax_langs --test ext_store --test ext_fetch --test git_cmd --test history

# property / fuzz tests, short run (256 cases each). Nightly CI runs test-prop-long.
test-prop:
	$(CARGO) test --test prop

test-prop-long:
	PROPTEST_CASES=$${PROPTEST_CASES:-20000} $(CARGO) test --release --test prop

# whole rendered screens at fixed sizes vs committed files in tests/snapshots/
test-snapshots:
	$(CARGO) test --test snapshots

update-snapshots:
	KURO_UPDATE_SNAPSHOTS=1 $(CARGO) test --test snapshots
	@echo "review the diff in tests/snapshots/ before committing"

# rendering and input on the cell buffer, plus the real binary in a pseudo-terminal (no database needed)
test-ui:
	$(CARGO) test --test ui_render --test ui_sql --test ui_sql_run --test ui_indent --test ui_modes --test ui_suggest --test ui_password --test ui_cursor --test ui_select_keys --test ui_mouse_select --test ui_ctrl_rightclick --test ui_hscroll --test ui_scrollbars --test ext_ksystem --test pty_ext --test git_ui --test pty_git --test ui_hardundo --test pty_hardundo --test pty_showinput \
	  --test color_rule --test pty_ui

# live sync: simulator, real sockets and locks, several editors, several real processes (SIGKILL of the leader)
test-sync:
	$(CARGO) test --lib sync
	$(CARGO) test --test sync_sim --test sync_net --test ui_sync --test pty_sync

# real MariaDB + PostgreSQL in throwaway Docker containers (skipped with a message if Docker is unavailable;
# set KURO_REQUIRE_DOCKER=1 to make that a failure, as CI does)
test-integration:
	$(CARGO) test --test integration_sql --test pty_sql -- --nocapture --test-threads=2

# regenerate the keybinding tables in docs/KEYBINDINGS.md from src/keymap.rs
docs:
	sh scripts/gen-keybindings.sh

# run everything and print passed / failed / skipped per test layer
report:
	sh scripts/test-report.sh

lint:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings

fmt:
	$(CARGO) fmt

clean:
	$(CARGO) clean
