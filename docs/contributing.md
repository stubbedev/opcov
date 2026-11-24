# Contributing

How to build opcov, run every test layer, add a PHP version, and write a
change the way this repository expects. How the extension works is in
[how-it-works.md](how-it-works.md); the binding layer is in
[ADR 0001](adr/0001-bindings.md).

## Layout

| Path | What it is |
|---|---|
| `crates/opcov-sys` | Bindings: bindgen over the target PHP's headers plus a C shim (`shim/`) of accessors for macros and inline functions |
| `crates/opcov-ext` | The extension (`cdylib`): module entry, compile hooks, probes, the `\opcov\` API. With `opcov-sys`, the only crate with `unsafe` |
| `crates/opcov-core` | Pure logic without PHP: CFG model and edge classification, outcome plan, file selection, report schema, merge |
| `crates/opcov-syntax` | Maps outcomes to source constructs with `mago-syntax` |
| `crates/opcov-cli` | The `opcov` command: `merge`, `convert`, `gate`, `doctor` |
| `php/phpunit` | The Composer package `opcov/phpunit` |
| `tests/phpt` | `.phpt` tests of the extension, one directory per area |
| `tests/diff`, `tools/measure` | The parity suite and the benchmark harness |
| `schema/` | The report's JSON Schema |
| `nix/` | Toolchain, packages, checks and dev shells |

## Toolchain and dev shell

Everything builds with nightly Rust: the dev shell, `nix build`, every
`nix flake check` check (and so CI) and rustfmt under treefmt. The toolchain
is defined once in `nix/build.nix`: the latest nightly that ships
rust-analyzer and rust-src, chosen by rust-overlay and pinned through
`flake.lock`, so Dependabot moves it forward. There is no stable fallback and
no MSRV; `Cargo.toml` declares no `rust-version`.

Every dev tool comes from the flake: the toolchain with rust-analyzer, clippy,
rustfmt and Miri, treefmt, cargo-nextest, cargo-shear, cargo-deny, cargo-fuzz,
bacon, sccache, just, jq, git, gh, mago, Composer, libclang and PHP with its
headers. Enter it
with `direnv allow` or `nix develop`.

The default shell has PHP 8.5 NTS. Every PHP build of the matrix has its own
shell, with `PHP_CONFIG`, php-src's `run-tests.php` (`PHP_RUN_TESTS`) and the
extensions the `.phpt` suite loads, all derived from that PHP:
`OPCOV_PCNTL_EXTENSION` (pcntl), `OPCOV_OPCACHE_EXTENSION` (OPcache, set only
for PHP 8.4, where it is a separate `opcache.so`; 8.5 and later build it in),
and `OPCOV_PCOV_EXTENSION` and `OPCOV_XDEBUG_EXTENSION` wherever nixpkgs does
not mark PCOV or Xdebug broken for that PHP. Tests that need an unset variable
skip:

```sh
nix develop .#php84 -c just phpt
nix develop .#php86-zts -c just phpt
```

The shells are `php84`, `php84-zts`, `php85`, `php85-zts`, `php86`,
`php86-zts`, and on Linux `php84-call-vm` (clang-built, CALL VM) and
`php85-tailcall-vm` (clang-built, TAILCALL VM). On x86_64 Linux, two more
shells hold instrumented PHP 8.5 builds for the sanitizer jobs
([below](#against-a-debug-php-src)): `php85-debug` and `php85-asan`.

### Fast iteration

- Dev and test builds of `opcov-cli` and `opcov-syntax` use the Cranelift
  backend (`.cargo/config.toml`; the toolchain in `nix/build.nix` ships
  `rustc-codegen-cranelift-preview`). Everything else stays on LLVM:
  Cranelift does not implement unwinding, so a `catch_unwind` compiled with it
  catches nothing and a panic passing through its frames skips their
  destructors ([ADR 0001](adr/0001-bindings.md)). That rules out `opcov-ext`,
  `opcov-sys` and `opcov-core`, whose code runs inside PHP under the
  extension's panic guard, and every third-party crate (`"*"`), since rayon,
  `std::thread::scope` and the extension's dependencies rely on unwinding.
  Release builds always use LLVM. Measured with two jobs and no sccache,
  Cranelift cuts a full rebuild of `opcov-cli` from 4.3 s to 3.5 s and of the
  `opcov-syntax` tests from 3.4 s to 2.8 s.
- Every build on Linux links with mold: crane's `stdenvSelector` in
  `nix/build.nix` picks `useMoldLinker` over `clangStdenv`, and the dev shell
  reuses that stdenv. macOS keeps clang's default linker, since mold cannot
  emit Mach-O.
- Every build runs rustc's parallel front end with eight threads
  (`-Zthreads=8` through `CARGO_BUILD_RUSTFLAGS` in `nix/build.nix`). A
  `RUSTFLAGS` set in the environment replaces it.
- `[profile.dev]` keeps only line tables as debuginfo for the workspace's own
  crates and none for dependencies.
- The dev shell wraps rustc in sccache (`RUSTC_WRAPPER`), which caches compiled
  dependencies across `cargo clean` and across projects. The Nix checks and CI
  do not use it: crane builds dependencies once into their own derivation.

### Profiles

`[profile.release]` uses thin LTO, one codegen unit and stripped symbols, and
unwinds on panic: the extension catches every panic at its engine entry
points so that PHP survives it, which `panic = "abort"` would make impossible.
`opcov-ext` refuses to compile under `panic = "abort"`.

## Building the extension

`opcov-sys`'s build script finds PHP through `php-config`: the one named by
`PHP_CONFIG`, else `php-config` on `PATH`. It accepts PHP 8.4, 8.5 and 8.6,
generates bindings with bindgen (which needs libclang) and compiles the C shim
against the same headers. It fails with a message naming the PHP it found when
that PHP is missing, unsupported or lacks its headers.

```sh
cargo build --release --package opcov-ext
php -d extension=target/release/libopcov_ext.so -r 'var_dump(\opcov\version());'
php -d extension=target/release/libopcov_ext.so --ri opcov
```

`--ri` prints the extension version, the PHP API it was built for, ZTS or NTS,
the VM kind of the running engine (HYBRID, CALL or TAILCALL) and whether
collection is available. On macOS the library is `libopcov_ext.dylib`.

The `engine-selftest` cargo feature adds `\opcov\engine_selftest()`,
`\opcov\selftest_panic()`, `\opcov\selftest_panic_in_probe()`,
`\opcov\selftest_force_watch()` and `\opcov\selftest_fail_snapshot()` for the `.phpt` tests under `engine/` and for
the outcome spike. Release builds never contain them.

### Against a debug php-src

A debug build of PHP enables the engine's `ZEND_DEBUG` assertions and reports
leaked memory at shutdown, which catches misuse of engine structures that a
release build tolerates. On x86_64 Linux the flake has two (`nix/php.nix`),
both PHP 8.5 NTS built with `--enable-debug` and unstripped, with their
extensions' own php-src test suites switched off:

- `php85-debug` (`phpDebug`): GCC, with Valgrind support, and `valgrind` in
  the shell. It uses PHP's bundled PCRE2 instead of the system one, because
  only the bundled copy is compiled with Valgrind support in a debug build:
  without it the PCRE JIT's SIMD scans read the padding after a string's
  terminating NUL, which memcheck reports as uninitialised reads from JIT code
  it cannot symbolize (every frame `???`). The shell sets `VALGRIND_OPTS`:
  `--leak-check=full`, without which memcheck's `-q` mode prints no leak at
  all; `--track-origins=yes` and `--keep-debuginfo=yes`, so a report names
  where the uninitialised value or the leaked block came from even after a
  module is unloaded; `--trace-children-skip=/bin/*,/sbin/*,/usr/*`, because
  `run-tests.php` traces child processes and a host executable such as
  `/bin/sh` (which `shell_exec()` and `passthru()` run) uses the host's
  dynamic linker, which on Ubuntu is stripped of the symbols memcheck needs to
  start, so such a child runs untraced, along with everything it spawns; and
  `--suppressions` with the suppression file in `nix/shell.nix`, one entry per
  known false positive, each with its justification. It also sets
  `USE_TRACKED_ALLOC=1`, which `run-tests.php -m` does not: with
  `USE_ZEND_ALLOC=0` PHP then frees every allocation a fatal error abandons,
  as the Zend allocator does, while a leak in a request that ends normally
  still reaches memcheck.
- `php85-asan` (`phpAsan`): clang, with `--enable-address-sanitizer
  --enable-undefined-sanitizer` and without Valgrind support (php-src refuses
  both together). The shell builds the extension and the CLI with
  `-Zsanitizer=address` and `-Zbuild-std` for the host target (with LLVM for
  every crate, since Cranelift has no sanitizers), compiles the C shim with the
  sanitizer flags php-src's configure adds, defines `ZEND_TRACK_ARENA_ALLOC`
  for the shim and bindgen as PHP's ASan build does (it changes the inline
  arena functions the shim calls), and sets `USE_ZEND_ALLOC=0`,
  `ZEND_DONT_UNLOAD_MODULES=1`,
  `ASAN_OPTIONS=detect_leaks=1:abort_on_error=1`,
  `UBSAN_OPTIONS=print_stacktrace=1:halt_on_error=1` and
  `TEST_PHP_ARGS=--asan` for `run-tests.php`.

```sh
nix develop .#php85-debug -c just phpt
nix develop .#php85-debug -c env TEST_PHP_ARGS="-m --show-mem" just phpt
nix develop .#php85-asan -c just phpt
```

The second line runs every test under Valgrind's memcheck (`run-tests.php -m`);
a test that leaks writes a `.mem` file, is reported as leaked and fails the
run. Each of these builds PHP and its extensions from source, which takes a
long time and several GB of memory; CI runs them (see [CI](#ci)).

To build a debug PHP outside Nix, for another version (8.4, 8.5 or 8.6) or ZTS,
use a php-src checkout and php-src's own build:

```sh
git clone --branch PHP-8.5 https://github.com/php/php-src
cd php-src
./buildconf --force
./configure --prefix="$HOME/php/8.5-debug" --enable-debug --enable-pcntl=shared
make -j"$(nproc)"
make install
```

Add `--enable-zts` for a thread-safe build. pcntl is built shared because
`tests/phpt/fork/fork.phpt` loads it from `OPCOV_PCNTL_EXTENSION`. Then build
and test opcov against it from the dev shell by pointing the variables at it:

```sh
export PHP_CONFIG="$HOME/php/8.5-debug/bin/php-config"
export PHP_RUN_TESTS="$PWD/php-src/run-tests.php"
export OPCOV_PCNTL_EXTENSION="$("$PHP_CONFIG" --extension-dir)/pcntl.so"
just phpt
```

The module entry records `ZEND_DEBUG` from the headers, so the extension built
this way loads only into that debug PHP, and a release extension does not load
into it. Building php-src takes several GB of memory and a long time; do it in
a capped scope (see [Running tests locally](#running-tests-locally)).

`nix/php-src.nix` builds a php-src revision with nixpkgs' (non-debug) PHP
recipe and runs the `.phpt` suite against it; the nightly workflow
(`.github/workflows/php-src-nightly.yml`) uses it on php-src `master`. Its
extension builds set `OPCOV_ALLOW_UNRELEASED_PHP=1` (`phptForPhpSrc` in
`nix/package.nix`), so `opcov-sys` accepts the unreleased PHP on `master`.

## Test layers

| Layer | Command | What it covers |
|---|---|---|
| Rust unit and integration tests | `just test` (`cargo nextest run`, profiles in `.config/nextest.toml`) | `opcov-core` (CFG model, classification, plan, merge, schema, including the golden CFG fixtures in `crates/opcov-core/tests/golden/cfg/`), `opcov-syntax`, `opcov-cli`, the extension's pure parts |
| Lints | `just lint`, `just fmt-check`, `just deny`, `just shear` | clippy at the levels below, treefmt, the dependency policy, unused dependencies |
| `.phpt` | `just phpt` | the extension against a real PHP, through php-src's `run-tests.php`: builds the extension with `engine-selftest` and the CLI, then runs `tests/phpt`. `tests/phpt/golden/cfg.phpt` checks that the CFG snapshots the extension builds match the golden fixtures |
| One `.phpt` area | `just phpt-run <extension> <paths>` | for example `just phpt-run target/debug/libopcov_ext.so tests/phpt/outcomes` |
| Miri | `just miri` | `opcov-core`'s tests under Miri (`-Zmiri-disable-isolation`, four proptest cases per property), without the `schema::` tests, whose third-party JSON Schema validator is too slow under Miri |
| Fuzzing | `just fuzz <target> <seconds>` | the `cargo fuzz` targets in `crates/opcov-core/fuzz` (`cfg_classify`, `report_parse`, `report_merge`) from their seed corpus, with a 2 GB RSS limit; crashing inputs land in `crates/opcov-core/fuzz/artifacts/` |
| Sanitizers | `nix develop .#php85-asan -c just phpt`, `nix develop .#php85-debug -c just phpt` | the `.phpt` suite under ASan and UBSan, in a debug PHP, and under Valgrind ([Against a debug php-src](#against-a-debug-php-src)) |
| PHP package | `just php-check` | builds the extension, runs `composer install` in `php/phpunit` and its fixture projects (PHPUnit 12, PHPUnit 13, Pest 4), then `mago analyze`, `mago lint`, `mago fmt --check` and the package's unit and integration tests with `OPCOV_EXTENSION` set to the built extension |
| Parity | `tools/measure/parity` | opcov against PCOV and Xdebug ([parity.md](parity.md)) |
| Benchmarks | `tools/measure/bench` | wall time, RSS and report size against the other drivers ([benchmarks.md](benchmarks.md)) |

`just check` runs fmt-check, lint, deny, shear, test, build and phpt in that
order. `just watch [job]` reruns a bacon job on every save. `just --list`
shows every recipe.

A `.phpt` test that has to start another PHP process must go through
`opcov_child()` in `tests/phpt/support/coverage.inc`. It strips the CGI
variables `run-tests.php` sets (`REDIRECT_STATUS`, `SCRIPT_FILENAME`); a
`php-cgi` child that inherits them runs the test file itself again, and the
process tree grows without bound.

### Running tests locally

Builds, the `.phpt` suite and anything that starts PHP run inside a
memory-capped scope with a timeout, so a runaway process kills only itself:

```sh
timeout 900 systemd-run --user --scope --quiet -p MemoryMax=6G -p MemorySwapMax=0 -p TasksMax=500 \
    nix develop -c env CARGO_BUILD_JOBS=2 just phpt
```

Scope builds and tests to the crate you change
(`cargo nextest run -p opcov-core`) instead of running the whole workspace on
every iteration. Leave the full matrix (`nix flake check`, php-src builds,
sanitizer runs, the parity corpus, the full benchmark matrix) to CI unless you
need it.

### CI

Every workflow is in `.github/workflows/`; `.github/actions/setup-nix`
installs Nix and restores the job's Nix store from the Actions cache, and
`.github/actions/plan` evaluates the workspace version, the flake's nightly as
a rustup toolchain name and the release image matrix. The job names are
stable, so any of them can be made a required check.

| Workflow | Trigger | Job | What it runs |
|---|---|---|---|
| `ci.yml` | every pull request, push to `main` | `flake-check` | `nix flake check` on x86_64 Linux, aarch64 Linux and arm64 macOS: treefmt's check, `just lint`, `just shear`, `just test` (nextest's `ci` profile), `just deny bans licenses sources` and, for every PHP build including the clang CALL and TAILCALL builds on Linux, the whole `.phpt` suite against the self-test build and `tests/phpt/smoke` against the release build |
| | | `php-package` | `nix develop -c just php-check` (Composer needs the network, which the Nix sandbox lacks) |
| | | `miri` | `nix develop -c just miri` |
| | | `plan`, `musl` | the musl build: `musl.yml` for every release image |
| `musl.yml` | called by `ci.yml` and `release.yml` | `build` | for every PHP image of the release matrix, on x86_64 and aarch64: `docker/musl.Dockerfile` in the official Alpine PHP image builds the extension with the flake's nightly, runs the whole `.phpt` suite against the self-test build and packages the release build as the musl archive |
| `sanitizers.yml` | push to `main`, nightly, manual | `asan-ubsan` | `nix develop .#php85-asan -c just phpt` on x86_64 Linux |
| | | `debug-valgrind` | on x86_64 Linux, `just phpt` in `php85-debug`, where a `ZEND_DEBUG` assertion or a leak report fails its test, then again under Valgrind (`TEST_PHP_ARGS="-m --show-mem"`), where any leak fails |
| `fuzz.yml` | nightly, manual | `targets`, `fuzz` | every target listed in `crates/opcov-core/fuzz/Cargo.toml`, 300 s each with `just fuzz`; on failure the crashing inputs are uploaded as `fuzz-artifacts-<target>` |
| `php-src-nightly.yml` | nightly, manual | `phpt`, `report` | the `.phpt` suite against php-src `master`, NTS and ZTS; a failure opens or comments on an issue |
| `security.yml` | every pull request, push to `main`, nightly | | CodeQL, Grype, `just deny advisories` and, on pull requests, dependency review |
| `release.yml` | `v*` tags | | checks the tag against `Cargo.toml` and `CHANGELOG.md`, builds and tests every release archive with Nix (`nix build .#release`) and on musl (`musl.yml`), pushes the Docker images and creates the GitHub release, with provenance attestations |

Dependabot keeps crates, workflows and flake inputs current.

### Nix outputs

Packages and checks are built with [crane](https://github.com/ipetkov/crane)
(`nix/package.nix`; the dev shells are `nix/shell.nix`; PHP builds are listed
once in `nix/php.nix`). `nix build` builds the `opcov` CLI;
`opcov-php84` ... `opcov-php86-zts` build the extension per PHP;
`php84-with-opcov` ... are PHP with opcov loaded; `release` builds every
release archive; `docker-*` build the layer images. `nix fmt` (or `just fmt`)
formats every file through treefmt (`nix/treefmt.nix`). Nix only sees files
tracked by git.

## Lints

`Cargo.toml`'s `[workspace.lints]` (every crate opts in with
`[lints] workspace = true`) denies clippy's `all`, `pedantic`, `cargo` and
`nursery` groups plus a set of `restriction` lints, and `just lint` passes
`-D warnings`:

- no panicking paths: `unwrap_used`, `expect_used`, `panic`, `unreachable`,
  `todo`, `unimplemented`, `indexing_slicing`, `string_slice`,
  `unwrap_in_result`, `panic_in_result_fn`, `get_unwrap`, `exit`;
- no swallowed errors: `map_err_ignore`, `let_underscore_must_use`,
  `unused_result_ok`, `assertions_on_result_states`;
- no silent suppressions: `allow_attributes`; every exception is an
  `#[expect(lint, reason = "...")]`, which errors once it no longer applies;
- no silent numeric bugs: `arithmetic_side_effects` (use `checked_*`,
  `saturating_*` or `wrapping_*`) and `as_conversions` (use `From`/`TryFrom`);
- deterministic output: `iter_over_hash_type`;
- explicit `return`: `implicit_return`.

`clippy.toml` lifts the panic, unwrap, expect, indexing and `dbg!` lints inside
tests. `unsafe_code` is denied; `opcov-sys` and `opcov-ext` opt in at their
crate roots, and every `unsafe` block carries a `// SAFETY:` comment
(`clippy::undocumented_unsafe_blocks`). `deny.toml` holds the dependency
policy: no known advisories or yanked crates, an allowlist of licences,
crates.io as the only source, no wildcard requirements and no duplicate crate
versions.

## Adding a PHP version

A new PHP minor is added at its first release candidate
([support.md](support.md)). The version is listed in these places:

1. `crates/opcov-sys/build.rs`: `PhpMinor` (accepted versions and their
   `php_version` cfg value).
2. `crates/opcov-ext/src/snapshot.rs`: `PHP`, from `PHP_VERSION_ID`.
3. `crates/opcov-core/src/cfg/opcode.rs`: a `PhpVersion` variant and
   `PhpVersion::ALL`; every new opcode in `Opcode` and in `BY_NUMBER`, by its
   number in `Zend/zend_vm_opcodes.h`, with the version that introduced it.
   An opcode opcov does not know decodes as `Unknown` and is reported in the
   snapshot diagnostics.
4. For each new or changed opcode, decide its role:
   `crates/opcov-core/src/cfg/classify.rs` (branching opcodes and their edge
   kinds), `crates/opcov-ext/src/lines.rs` (ignored opcodes and consumers,
   which can differ per version, as `BOOL` does from 8.5), and
   `crates/opcov-ext/src/branches.rs` (`Decision`: evaluated or watched).
5. Engine API changes go into the C shim (`crates/opcov-sys/shim/`), as
   `zend_ini_string_ex`'s `orig` parameter did between 8.5 and 8.6.
6. The golden CFG fixtures: the `php` list of each
   `crates/opcov-core/tests/golden/cfg/*.json` and `SUPPORTED_VERSIONS` in
   `crates/opcov-core/tests/cfg_golden.rs`.
7. `nix/php.nix`: `phpBuilds` (NTS and ZTS), which drives the packages, the
   dev shells, the `.phpt` checks and the release archives, and
   `phpOfficialImages` for the Docker images.
8. The `php` constraint in `composer.json` (PIE) and in
   `php/phpunit/composer.json`.
9. `docs/support.md`, `docs/install.md` and `CHANGELOG.md`.

Then run `just test` and `just phpt` in the new version's dev shell.

## Writing a change

- **Commits.** One logical change per commit. The subject is a plain
  imperative sentence ("Record every branch outcome and loop iteration count
  with fire-once probes"), without a conventional-commit prefix. The body
  explains why: the problem, the decision and the evidence (numbers, the test
  that proves it), and ends with `Closes #N` or `Refs #N` for the issues it
  touches.
- **No comments in source.** Code states its intent through names and types;
  rationale goes into the commit message, an ADR (`docs/adr/`) or these docs.
  The only comments are functional: `// SAFETY:` on `unsafe` blocks.
- **Tests with every change.** A feature lands with the tests that show its
  behaviour; a bug fix lands with a test that failed before it. A refusal
  message gets an entry in [troubleshooting.md](troubleshooting.md).
- **Decisions** that change the mechanism or the report get an ADR.
- **The changelog** (`CHANGELOG.md`) is written by hand when a change lands.
