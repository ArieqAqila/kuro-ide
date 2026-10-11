#!/bin/sh
# CI / local full run:  ./scripts/ci.sh    (fish: sh scripts/ci.sh)
# Set KURO_REQUIRE_DOCKER=1 to fail instead of skipping the Docker-based tests.
set -eu
cd "$(dirname "$0")/.."
cargo fmt --check
cargo build --all-targets
make test-unit
make test-ui
make test-integration
