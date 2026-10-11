# Works the same from fish, bash or zsh:  make test
CARGO ?= cargo

.PHONY: build install test test-unit test-ui test-integration lint fmt clean

build:
	$(CARGO) build --release

install:
	$(CARGO) install --path . --locked
	@mkdir -p $(HOME)/.config/fish/completions
	cp completions/kuro.fish $(HOME)/.config/fish/completions/kuro.fish

# everything, in order; stops at the first failing group
test: test-unit test-ui test-integration

# pure logic: find, buffer, table layout, error classification (recorded fixtures), config, docs check
test-unit:
	$(CARGO) test --lib --test classify_fixtures --test docs_check

# rendering/input on the cell buffer + the real binary in a pseudo-terminal (no database needed)
test-ui:
	$(CARGO) test --test ui_render --test ui_sql --test pty_ui

# real MariaDB + PostgreSQL in throwaway Docker containers (skipped with a message if Docker is unavailable;
# set KURO_REQUIRE_DOCKER=1 to make that a failure, as CI does)
test-integration:
	$(CARGO) test --test integration_sql --test pty_sql -- --nocapture --test-threads=2

lint:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings

fmt:
	$(CARGO) fmt

clean:
	$(CARGO) clean
