# opcov dev tasks.

set shell := ["bash", "-euo", "pipefail", "-c"]
set script-interpreter := ["bash", "-euo", "pipefail"]

php_config := env("PHP_CONFIG", "php-config")

# List all recipes with their descriptions.
default:
    @just --list

# Run every release gate in order: fmt-check, lint, deny, shear, test, build, phpt.
check: fmt-check lint deny shear test build phpt

# Fail when any file is not formatted per nix/treefmt.nix.
fmt-check:
    treefmt --ci

# Lint every target; the lint levels live in Cargo.toml's [lints].
lint:
    cargo clippy --all-targets --locked -- -D warnings

# Check deny.toml's policy: every check by default, or a subset, e.g. `just deny advisories`.
deny *checks:
    cargo deny --locked check {{ checks }}

# Fail on dependencies no target uses.
shear:
    cargo shear --locked --deny-warnings --check-test-targets

# Run the test suite with cargo-nextest; profiles live in .config/nextest.toml.
test:
    cargo nextest run --locked

# Compile-check every target.
build:
    cargo build --locked

# Build the extension with its engine self-test and run the whole .phpt suite against php-config's PHP.
phpt:
    extension="$(cargo build --locked --package opcov-ext --features engine-selftest --message-format=json-render-diagnostics | jq -r 'select(.reason == "compiler-artifact" and .target.name == "opcov_ext") | .filenames[]')" && just phpt-run "$extension" tests/phpt

# Run the .phpt tests under `paths` with php-src's run-tests.php, loading the extension at `extension`.
[script]
phpt-run extension +paths:
    php="$({{ php_config }} --php-binary)"
    runner="$(mktemp -d)"
    trap 'rm -r "$runner"' EXIT
    cp "{{ env('PHP_RUN_TESTS') }}" "$runner/run-tests.php"
    "$php" "$runner/run-tests.php" -n -q --show-diff -p "$php" -d "extension={{ extension }}" {{ paths }}
