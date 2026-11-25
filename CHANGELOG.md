# Changelog

Every notable change to opcov is recorded here, newest first. The format
follows [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/), and
versions follow [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html).
[docs/support.md](docs/support.md) says what a version number promises and which
PHP versions each release supports.

The release workflow publishes the section whose heading matches the pushed tag
as the release notes, and refuses to release a version that has no section here.

## [Unreleased]

## [0.2.0] - 2026-11-25

### Added

- `opcov consumers` finds the code that uses changed PHP members in process
  with mago, without running PHP. A use is matched by the receiver's type,
  including the code the framework calls: queued jobs, event subscribers,
  model events and console commands. The hooks come from the code itself:
  observers, `static::<event>()` closures, `$dispatchesEvents` and command
  names, so a request names only the changed members.
  - **Diffs and depth:** `--diff` takes the changed members from a diff,
    `--depth` follows the call graph level by level, and `--format files`
    lists the consumers for `opcov gate --files @-`.
  - **Laravel:** the types Laravel resolves at runtime are declared to the
    analyzer: model builders and static forwarding, scopes, dynamic wheres,
    relation properties, facades and root aliases.
  - **Indexing:** only the vendor code the project reaches through Composer's
    autoload maps is indexed.

### Fixed

- `opcov\get_debug_info()` keys the functions of `eval()`'d code by the
  entry's reported name, with its `#<digest>` suffix, as `opcov\executable()`
  and the report do; it left the suffix out of `{main}`, so the two did not
  match.

## [0.1.4] - 2026-11-25

### Fixed

- `opcov doctor` reports a ZTS PHP as ZTS. It compared `PHP_ZTS`, a bool on
  every supported PHP, with `1`, so the printed runtime and the JSON
  `runtime.zts` always said NTS.

## [0.1.3] - 2026-11-25

### Added

- `opcov stitch` lays a run of the tests a change reaches over the baseline
  report and writes the report a run of every test on the branch would write.
  `opcov impact --whole-files` selects the tests it needs: also those that ran
  a changed file. It refuses unbounded changes, a partial run without a
  selected test, a baseline test left with hits in a changed file, and reports
  of another tree.

## [0.1.2] - 2026-11-25

### Added

- `opcov impact` reads a baseline report and a diff, shows the baseline
  coverage of every changed function and the tests that reached it, and
  selects the tests that must run to measure the change, as text, JSON or a
  PHPUnit/Pest `--filter` expression. It rebuilds each changed file's base
  version from the diff and refuses a baseline whose hashes do not match, and
  it exits 1 for changes coverage cannot bound (declarations outside function
  bodies, test helpers, non-PHP files).

- Every release carries the `opcov` command as a native binary for x86_64 and
  aarch64 Linux (static, any libc) and macOS arm64, with checksums and
  attestations. `opcov/phpunit` is now an optional Composer plugin that
  downloads and verifies the binary for its version and installs it as
  `vendor/bin/opcov`, which also fetches it on first use when the plugin is not
  allowed.

### Changed

- The diff parser of `opcov gate` reads git's rename, copy, new-file,
  deleted-file and binary headers, so a rename without content changes and a
  binary file are recognised instead of skipped silently; the functions it
  checks are unchanged.

## [0.1.1] - 2026-11-24

### Fixed

- The aarch64 musl archives did not load in the official Alpine PHP images
  (`Error loading shared library libgcc_s.so.1`). The musl builds now link
  libgcc statically, so `opcov.so` depends on musl's libc only, and every musl
  archive is loaded in a clean official image before it is released.
- Hash-table access in the extension no longer dereferences a pointer that may
  be null (CodeQL `rust/access-invalid-pointer`).

### Added

- Pest 5 support in `opcov/phpunit`, tested alongside Pest 4 and PHPUnit 12/13.
- `opcov/phpunit` is published on Packagist from the read-only split
  `stubbedev/opcov-phpunit`; `opcov/opcov` installs with `pie install`, which
  every release now smoke-tests on x86_64 and aarch64 Linux (glibc, musl, ZTS)
  and macOS arm64.

## [0.1.0] - 2026-11-24

First release: opcode-level line, branch-outcome and loop coverage for PHP
8.4, 8.5 and 8.6 (NTS and ZTS) on Linux (glibc and musl, x86_64 and aarch64)
and macOS arm64.

### Added

- **The extension** (`opcov.so`), on opcov's own Zend bindings (bindgen over
  the target PHP's headers plus a C shim). Fire-once probes are swapped into
  opcode handlers and restore the real handler once they have seen everything,
  so measured code returns to full speed; HYBRID, CALL and TAILCALL VMs.
- **Coverage:** executable lines (PCOV's rules); every outcome of every
  branching construct (`if`/`elseif`/`else` including the implicit `else`,
  `&&`/`||`/`and`/`or`, `??`, `??=`, `?:`, `?->`, `match` arms including
  jump tables and no-arm, `switch` cases and default, `catch`, `finally` from
  normal flow and from an exception); loops that never ran zero times or never
  ran twice, for `foreach` (arrays, iterators, generators), `for`, `while`,
  `do`-`while`, including loops that only continue through a `catch`.
  `docs/branch-semantics.md` defines every outcome.
- **Collection API:** `\opcov\start()`, `stop(?string $test)`, `reset()`,
  `executable()`, `write_report()`, `ensure_can_collect()`, `get_status()`,
  `get_debug_info()`, with per-test attribution whose cost scales with the
  probes that fired.
- **File selection** (`opcov.collect_paths`, `opcov.exclude_paths`,
  `opcov.exclude_pattern`), decided once per compiled file; unselected files
  run as without opcov.
- **The canonical report** `opcov-report/1` (`docs/report-schema.md`),
  streamed by one writer and reader, with lossless encoding of names and paths
  that are not UTF-8, sha256 of every measured source, and one entry per
  distinct `eval()`'d source.
- **The `opcov` CLI:** `merge` (byte-reproducible, bounded memory), `convert`
  (lcov, Clover, Cobertura, HTML), `gate` (changed-code coverage from a diff,
  with exit codes for CI) and `doctor` (environment self-test and source
  mapping check). `docs/cli.md`, `docs/gate.md`.
- **Source mapping** of outcomes to constructs and exact spans through
  mago-syntax (99.94% of outcomes on a 4.97M-line corpus).
- **`opcov/phpunit`** for PHPUnit 12/13 and Pest 4: an extension, a launcher
  that sets the INI from PHPUnit's `<source>`, paratest support, a PHPUnit 13.3
  coverage driver with branch coverage, and a `.cov` converter.
  `docs/phpunit.md`.
- **Refusals with stable codes** for every unsupported setup (OPcache
  optimizer, JIT, file cache, preload, protected memory, OPcache under a
  shared SAPI, Xdebug or PCOV in coverage mode), each with an entry in
  `docs/troubleshooting.md`; per-file failures skip only that file.
- **Packaging:** release archives named for PIE, Docker layer images, Nix
  packages and an overlay. `docs/install.md`.

### Performance

Instructions relative to no coverage (`docs/benchmarks.md`): about 1.02x on a
Laravel package suite and 1.48x on Carbon's suite with full line, branch and
loop coverage, below PCOV's line-only overhead; on a real branch the
changed-code gate's coverage step is about 314x faster than Xdebug path
coverage. On a 4.97M-line corpus, compiling with every file selected takes
about 1.6x plain compilation (`docs/scale.md`).

[Unreleased]: https://github.com/stubbedev/opcov/compare/v0.2.0...main
[0.2.0]: https://github.com/stubbedev/opcov/compare/v0.1.4...v0.2.0
[0.1.4]: https://github.com/stubbedev/opcov/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/stubbedev/opcov/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/stubbedev/opcov/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/stubbedev/opcov/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/stubbedev/opcov/releases/tag/v0.1.0
