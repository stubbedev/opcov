# Changelog

Every notable change to opcov is recorded here, newest first. The format
follows [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/), and
versions follow [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html).
[docs/support.md](docs/support.md) says what a version number promises and which
PHP versions each release supports.

The release workflow publishes the section whose heading matches the pushed tag
as the release notes, and refuses to release a version that has no section here.

## [Unreleased]

### Added

- The `opcov` PHP extension (`opcov.so`), built on opcov's own Zend engine
  bindings (`opcov-sys`: bindgen over the target PHP's headers plus a C shim of
  accessors). It loads on PHP 8.4, 8.5 and 8.6, NTS and ZTS, on the HYBRID, CALL
  and TAILCALL VMs. `\opcov\version()` returns the extension version, and
  `php --ri opcov` shows the version, PHP API, thread safety, VM kind and
  whether collection is available. See `docs/adr/0001-bindings.md`.
- A panic guard at every engine entry point: a panic in the extension disables
  collection with the reason and leaves PHP running.
- The CFG snapshot model and the classification of every control-flow edge into
  an outcome kind, shared by the extension and the reports.
- The rules that decide which compiled files get probes, from
  `opcov.collect_paths` and `opcov.exclude_paths` (`:`-separated paths or
  `@<listfile>`), with exclusion always winning.
- The mapping of outcomes to the source constructs they come from (`if`, `?:`,
  `??`, `?->`, `&&`/`||`, `match`, `switch`, loops, `catch`, `finally`), with
  exact spans, through mago-syntax at report time.
- The canonical report schema `opcov-report/1` (`schema/opcov-report.v1.json`,
  documented in `docs/report-schema.md`) with a validating reader.
- `opcov merge`, which merges per-worker reports into the report a serial run
  would produce, byte-identically for any input order and in bounded memory.
- Packaging: release archives per PHP minor, thread safety and platform, named
  as PIE expects for `pie install opcov/opcov`; Docker images for
  `COPY --from`; Nix packages for each PHP build, `php*-with-opcov` PHP
  packages, the `opcov` CLI and an overlay. See `docs/install.md`.
- Release automation: a tag `v<version>` builds, tests, packages and attests
  every artifact and publishes the GitHub release with this changelog's
  section; a nightly job builds against php-src `master`.

[Unreleased]: https://github.com/stubbedev/opcov/commits/main
