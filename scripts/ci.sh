#!/bin/sh
# CI / local full run:  sh scripts/ci.sh        (what the "ci" workflow does)
# Set KURO_REQUIRE_DOCKER=1 to fail instead of skipping the Docker-based tests.
set -eu
cd "$(dirname "$0")/.."
make lint
make test-unit
make test-prop
make test-snapshots
make test-ui
make test-integration
