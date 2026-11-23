# opcov

`src/lib.rs` holds the logic and its tests; `src/main.rs` only does I/O and
calls into the library, which keeps the core pure and lets integration tests
under `tests/` use it.

## Recipes

| Recipe | What it does |
| --- | --- |
| `just check` | Every release gate in order: fmt-check, lint, deny, shear, test, build, phpt |
| `just fmt-check` | Fail when any file is not formatted (`treefmt --ci`) |
| `just lint` | `cargo clippy` against the strict levels in `Cargo.toml` |
| `just deny [checks]` | `cargo deny check` against `deny.toml`: every check, or a subset such as `advisories` |
| `just shear` | `cargo shear`: fail on dependencies no target uses |
| `just test` | `cargo nextest run` (profiles in `.config/nextest.toml`) |
| `just build` | Compile-check every target |
| `just phpt` | Build the extension with its engine self-test and run the `.phpt` suite against `php-config`'s PHP |
| `just phpt-run <extension> <paths>` | Run the `.phpt` tests under `paths` with php-src's `run-tests.php`, loading the given extension |
| `just fmt` | Format every file in place (`treefmt`) |
| `just watch [job]` | Rerun a bacon job on every save (`clippy-all` by default, or `nextest`, `run`, ...) |

## The PHP extension

The extension is the `cdylib` crate `crates/opcov-ext`, built on the Zend
bindings in `crates/opcov-sys` (see `docs/adr/0001-bindings.md` for the choice
and every engine symbol it uses). It supports PHP 8.4, 8.5 and 8.6, NTS and
ZTS.

### Build

`opcov-sys`'s build script finds PHP through `php-config`: the one named by
`PHP_CONFIG`, else `php-config` on `PATH`. It generates bindings with bindgen,
which needs libclang, and compiles a small C shim against the same headers. The
build fails with a message naming the PHP it found when that PHP is missing,
unsupported or lacks its headers. The dev shell provides PHP 8.5 with its
headers (`PHP_CONFIG`), libclang and php-src's `run-tests.php`
(`PHP_RUN_TESTS`), so in `nix develop`:

```sh
cargo build --release --package opcov-ext
php -d extension=target/release/libopcov_ext.so -r 'var_dump(\opcov\version());'
php -d extension=target/release/libopcov_ext.so --ri opcov
```

`--ri` prints the extension version, the PHP API it was built for, ZTS or NTS,
the VM kind of the running engine (HYBRID, CALL or TAILCALL) and whether
collection is available. To build against another PHP, point `PHP_CONFIG` at
its `php-config`. On macOS the library is `libopcov_ext.dylib`.

Nix builds the extension per PHP: `nix build .#opcov-php84`, `opcov-php84-zts`,
`opcov-php85`, `opcov-php85-zts`, `opcov-php86`, `opcov-php86-zts`, each
installing `lib/php/extensions/opcov.so`.

### Test

`.phpt` tests live in `tests/phpt` (`smoke/` for the PHP-facing surface,
`engine/` for engine internals) and run under php-src's `run-tests.php`.
`just phpt` builds the extension with the `engine-selftest` feature, which adds
`\opcov\engine_selftest()` and `\opcov\selftest_panic()` for the `engine/`
tests, and runs the whole suite against the dev shell's PHP. The release package
never contains those functions; `nix flake check` runs the whole suite against
the self-test build and `smoke/` against the release build, for every PHP above,
plus clang-built PHP 8.4 (CALL VM) and 8.5 (TAILCALL VM) on Linux.

## Toolchain

Everything builds with nightly Rust: the dev shell, `nix build`, every
`nix flake check` check (and so CI) and rustfmt under treefmt. The
toolchain is defined once in `nix/toolchain.nix`: the latest nightly that
ships rust-analyzer and rust-src, chosen by rust-overlay and pinned through
`flake.lock`, so Dependabot moves it forward. There is no stable fallback
and no MSRV; `Cargo.toml` declares no `rust-version`.

Every dev tool (that toolchain with rust-analyzer, clippy and rustfmt, plus
treefmt, cargo-nextest, cargo-shear, bacon, cargo-deny, just, jq,
git and gh, plus PHP 8.5 with its headers and libclang) comes from the flake: `direnv allow` on entry, or `nix develop`
explicitly.

## Fast iteration

- Every build uses the LLVM backend. Cranelift would compile unoptimised code
  faster, but it does not implement unwinding, so the extension's
  `catch_unwind` guards would catch nothing and a panic would abort PHP (see
  `docs/adr/0001-bindings.md`).
- Every build on Linux links with mold, which cuts most of an incremental
  rebuild's time: crane's `stdenvSelector` in `nix/toolchain.nix` picks
  `useMoldLinker` over `clangStdenv`, and the dev shell reuses that stdenv.
  macOS keeps clang's default linker, since mold cannot emit Mach-O.
- Every build runs rustc's parallel front end with eight threads
  (`-Zthreads=8` through `CARGO_BUILD_RUSTFLAGS` in `nix/toolchain.nix`);
  the front end is single-threaded otherwise. A `RUSTFLAGS` set in the
  environment replaces it.
- `[profile.dev]` keeps only line tables as debuginfo for the workspace's
  own crates and none for dependencies: backtraces still carry file and
  line in your code, but compiling and linking emit far less.
- The dev shell wraps rustc in sccache (`RUSTC_WRAPPER`), which caches
  compiled dependencies across `cargo clean` and across projects. Workspace
  crates compile incrementally, which sccache does not cache, so it passes
  them straight to rustc. The Nix checks and CI do not use it: crane already
  builds dependencies once into their own derivation.

## Profiles

`[profile.release]` uses thin LTO, one codegen unit and stripped symbols, and
unwinds on panic: the extension catches every panic at its engine entry points
so that PHP survives it, which `panic = "abort"` would make impossible.
`opcov-ext` refuses to compile under `panic = "abort"`.

## Nix

The package and its checks are built with [crane](https://github.com/ipetkov/crane)
(`nix/package.nix`; the dev shell is `nix/shell.nix`, both imported by
`flake.nix`). Dependencies build once into their own derivation, so editing
the crate's own code never rebuilds them.

- `nix build` builds the release package; name, version and description
  come from `Cargo.toml`, dependencies from `Cargo.lock`
- `nix flake check` runs treefmt's check, `just lint`, `just shear`,
  `just test` (nextest's `ci` profile), `just deny bans licenses sources`
  and the `.phpt` suite for every PHP build (`phpt-*`) in the sandbox, so the justfile stays the one definition of each gate.
  Advisories need network access the sandbox lacks, so the security workflow
  checks them instead.
- `nix run` runs it
- `nix fmt` (or `just fmt`) formats every file through treefmt
  (`nix/treefmt.nix`): alejandra for Nix, rustfmt for Rust, taplo for TOML,
  yamlfmt for YAML and `just --fmt` for the justfile, plus actionlint over the
  workflows

## CI

`.github/workflows/ci.yml` runs `nix flake check` on every pull request and
push to `main`, on x86_64 Linux (`ubuntu-24.04`), aarch64 Linux
(`ubuntu-24.04-arm`) and arm64 macOS (`macos-latest`). Jobs that need Nix use `.github/actions/setup-nix`, which
installs it and restores the job's Nix store from the Actions cache, keyed on the
runner's OS and architecture, `flake.lock`, `Cargo.lock` and `nix/`, so crane's dependency build and the
toolchain are not rebuilt or refetched on every run. `.github/workflows/security.yml` runs CodeQL (Rust and GitHub
Actions), Grype and `just deny advisories` in the nix dev shell, also nightly,
uploading CodeQL and Grype findings to code scanning; dependency review gates
pull requests. Dependabot keeps the crates, the workflows and the flake inputs (and with
them the toolchain and every dev tool) current.

## Linting

`Cargo.toml`'s `[workspace.lints]` (which the root crate opts into with
`[lints] workspace = true`, as must every crate added to the workspace) denies clippy's `all`, `pedantic`, `cargo` and `nursery` groups
plus a curated set of `restriction` lints, and `just lint`
passes `-D warnings`, so every finding fails the build. The extra lints:

- forbid panicking paths: `unwrap_used`, `expect_used`, `panic`,
  `unreachable`, `todo`, `unimplemented`, `indexing_slicing`, `string_slice`,
  `unwrap_in_result`, `panic_in_result_fn`, `get_unwrap`, `exit`
- forbid swallowed errors: `map_err_ignore`, `let_underscore_must_use`,
  `unused_result_ok`, `assertions_on_result_states`
- forbid silent suppressions: `allow_attributes` and
  `allow_attributes_without_reason`, so every exception is an
  `#[expect(lint, reason = "...")]` that errors once it no longer applies
- forbid silent numeric bugs: `arithmetic_side_effects` (use `checked_*`,
  `saturating_*` or `wrapping_*`) and `as_conversions` (use `From`/`TryFrom`)
- keep output deterministic: `iter_over_hash_type`
- require explicit `return`: `implicit_return` (`needless_return`
  contradicts it and is allowed)
- `nursery` lints can report false positives; answer those with an `#[expect]`

`clippy.toml` lifts the panic, unwrap, expect, indexing and `dbg!` lints
inside tests, where panicking is how a test fails. `unsafe_code` is denied;
`opcov-sys` and `opcov-ext` opt in at their crate roots with an `#[expect]`, and
every `unsafe` block carries a `// SAFETY:` comment
(`clippy::undocumented_unsafe_blocks`).

`deny.toml` holds the dependency policy `just deny` enforces, over the
dependency graph of the Linux and macOS targets in `[graph] targets` only: no known
advisories or yanked crates, an allowlist of licences, crates.io as the only
source, no wildcard requirements and no duplicate crate versions.
Duplicates are checked there instead of by clippy's `multiple_crate_versions`
because `[bans] skip` can exempt one crate at a time.
