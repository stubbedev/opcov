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

# Run opcov-core's tests under Miri with four proptest cases per property; the schema tests are skipped, since the third-party JSON Schema validator they drive is too slow under Miri.
miri:
    MIRIFLAGS=-Zmiri-disable-isolation PROPTEST_CASES=4 cargo miri test --package opcov-core --locked -- --skip schema::

# Fuzz the opcov-core target `target` for `seconds` seconds from the checked-in seed corpus, which stays untouched; crashing inputs land in crates/opcov-core/fuzz/artifacts.
[script]
fuzz target seconds:
    corpus="$(mktemp -d)"
    trap 'rm -r "$corpus"' EXIT
    cargo fuzz run --fuzz-dir crates/opcov-core/fuzz --target-dir "${CARGO_TARGET_DIR:-{{ justfile_directory() }}/target}/fuzz" "{{ target }}" "$corpus" "crates/opcov-core/fuzz/corpus/{{ target }}" -- -max_total_time={{ seconds }} -rss_limit_mb=2048

# Build the extension with its engine self-test and the opcov CLI, and run the whole .phpt suite against php-config's PHP.
[script]
phpt:
    artifacts="$(cargo build --locked --package opcov-ext --features engine-selftest --package opcov-cli --message-format=json-render-diagnostics)"
    extension="$(jq -r 'select(.reason == "compiler-artifact" and .target.name == "opcov_ext") | .filenames[]' <<<"$artifacts")"
    OPCOV_CLI="$(jq -r 'select(.reason == "compiler-artifact" and .target.name == "opcov") | .executable' <<<"$artifacts")"
    export OPCOV_CLI
    just phpt-run "$extension" tests/phpt

# Run the .phpt tests under `paths` with php-src's run-tests.php, loading the extension at `extension`.
[script]
phpt-run extension +paths:
    php="$({{ php_config }} --php-binary)"
    runner="$(mktemp -d)"
    trap 'rm -r "$runner"' EXIT
    cp "{{ env('PHP_RUN_TESTS') }}" "$runner/run-tests.php"
    "$php" "$runner/run-tests.php" -n -q --show-diff -p "$php" -d "extension={{ extension }}" {{ paths }}

# Package the extension at `extension` as the release archive for php-config's PHP into `out`, named as PIE expects, and run the smoke tests against the packaged copy.
[script]
dist extension version libc out:
    case "{{ libc }}" in glibc | musl | bsdlibc) ;; *) echo "dist: libc must be glibc, musl or bsdlibc, got '{{ libc }}'" >&2; exit 1 ;; esac
    php="$({{ php_config }} --php-binary)"
    asset="$("$php" -n -r 'function fail(string $message): never { fwrite(STDERR, "dist: " . $message . "\n"); exit(1); } if (PHP_EXTRA_VERSION !== "") { fail("PHP " . PHP_VERSION . " is a pre-release; release archives are built against released PHP versions only"); } $machine = php_uname("m"); $arch = ["x86_64" => "x86_64", "aarch64" => "arm64", "arm64" => "arm64"][$machine] ?? fail("unsupported machine " . $machine); printf("php_opcov-v%s_php%d.%d-%s-%s-%s-%s", $argv[1], PHP_MAJOR_VERSION, PHP_MINOR_VERSION, $arch, strtolower(PHP_OS_FAMILY), $argv[2], PHP_ZTS ? "zts" : "nts");' "{{ version }}" "{{ libc }}")"
    work="$(mktemp -d)"
    trap 'rm -r "$work"' EXIT
    mkdir "$work/stage" "$work/check"
    install -m 0644 "{{ extension }}" "$work/stage/opcov.so"
    install -m 0644 LICENSE "$work/stage/LICENSE"
    (cd "$work/stage" && sha256sum opcov.so LICENSE > SHA256SUMS)
    mkdir -p "{{ out }}"
    tar --sort=name --owner=0 --group=0 --numeric-owner --mtime=@0 -C "$work/stage" -c opcov.so LICENSE SHA256SUMS | gzip -9n > "{{ out }}/$asset.tgz"
    tar -xzf "{{ out }}/$asset.tgz" -C "$work/check"
    (cd "$work/check" && sha256sum -c SHA256SUMS)
    just phpt-run "$work/check/opcov.so" tests/phpt/smoke
    (cd "{{ out }}" && sha256sum "$asset.tgz" > "$asset.tgz.sha256")

# Print the CHANGELOG.md section for `version`, failing when it has none.
changelog version:
    @awk -v heading="## [{{ version }}]" 'index($0, "## ") == 1 { if (found) exit; found = index($0, heading) == 1; next } found && /^\[[^]]+\]: / { exit } found { print } END { if (!found) { print "changelog: CHANGELOG.md has no section " heading > "/dev/stderr"; exit 1 } }' CHANGELOG.md

# Check the opcov/phpunit package: mago analysis, lint and format, then its unit and integration tests against the built extension.
[script]
php-check:
    artifacts="$(cargo build --locked --package opcov-ext --message-format=json-render-diagnostics)"
    OPCOV_EXTENSION="$(jq -r 'select(.reason == "compiler-artifact" and .target.name == "opcov_ext") | .filenames[]' <<<"$artifacts")"
    export OPCOV_EXTENSION
    cd php/phpunit
    composer install --no-interaction --no-progress --quiet
    for fixture in tests/fixture/phpunit12 tests/fixture/phpunit13 tests/fixture/pest4; do
        composer install --no-interaction --no-progress --quiet --working-dir="$fixture"
    done
    mago analyze
    mago lint
    mago fmt --check
    php vendor/bin/phpunit
