#!/usr/bin/env bash
set -euo pipefail

cargo test -p integration-tests -- --test-threads=1 --nocapture