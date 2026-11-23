# Versioning and support policy

## What is versioned

opcov has two version numbers, each following
[Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html):

| Artifact | Version | Where it is defined |
|---|---|---|
| The extension (`opcov.so`), the `opcov` CLI and the PHP package `opcov/phpunit` | `<major>.<minor>.<patch>`, one number for all three | `[workspace.package] version` in `Cargo.toml`; a release is the tag `v<version>`; the PHP package's version constant is checked against it by its own tests |
| The report schema | `opcov-report/<major>` | `docs/report-schema.md`, section "Version policy" |

For the extension and the CLI, a major version changes when an existing user
has to change something: a `\opcov\` function or its result shape, an INI
setting, a CLI flag or exit code, or the oldest report schema the CLI reads. A
minor version adds features, new PHP minors or platforms. A patch fixes bugs.
Removing a PHP minor that php.net no longer supports happens in a minor release.
Before 1.0.0, a minor release may also break.

The report schema records only its major version; additive changes keep it, and
readers reject a major version they do not know. The schema version does not
follow the extension's version. `docs/report-schema.md` holds the details.

The PHP package checks the loaded extension at startup and refuses to run
unless both are on the same compatibility line: the same major version, or
the same minor version before 1.0.0. After writing a report it also checks
that the file declares `opcov-report/1`.

## Supported PHP versions

opcov supports the PHP versions that php.net supports, in active support or
security support (see [php.net/supported-versions](https://www.php.net/supported-versions.php)),
both NTS and ZTS, non-debug builds.

| PHP | Supported by opcov | php.net support ends |
|---|---|---|
| 8.4 | yes | 31 Dec 2028 |
| 8.5 | yes | 31 Dec 2029 |
| 8.6 | yes | 31 Dec 2030 |

- A new PHP minor is added at its first release candidate (RC1): opcov builds
  and passes its tests against it from then on, and the first opcov release
  after its general availability ships artifacts for it.
- A PHP minor is dropped in the first opcov minor release after php.net ends
  its security support.
- php-src `master` is built every night (`.github/workflows/php-src-nightly.yml`);
  a failure opens an issue titled "Nightly build against php-src master fails",
  or comments on the open one.

## Supported platforms

| Platform | Artifacts |
|---|---|
| Linux x86_64 and arm64, glibc 2.34 or newer | release archives, PIE, Docker images, Nix |
| Linux x86_64 and arm64, musl (Alpine) | release archives, PIE |
| macOS arm64 | release archives, PIE, Nix |

Windows is not supported. `docs/install.md` lists every channel.

## Release notes

Each release's notes are its section in `CHANGELOG.md`, which is written by
hand when a change lands, plus the PHP versions it was built for and the report
schema version it writes.

## OPcache and JIT

opcov checks these settings when a request starts and on every collection call.
A refused setup installs nothing and makes `\opcov\ensure_can_collect()` throw
with the setting, the value to use and a link to its entry in
[troubleshooting.md](troubleshooting.md). [ADR 0004](adr/0004-opcache-jit.md)
explains each verdict.

| Setup | Supported | Use instead |
|---|---|---|
| OPcache off (not loaded, `opcache.enable=0`, or `opcache.enable_cli=0` in the CLI) | yes | |
| CLI OPcache without the optimizer (`opcache.enable_cli=1`, `opcache.optimization_level=0`) | yes | |
| Optimizer (`opcache.optimization_level` other than 0, the default is `0x7FFEBFFF`) | no | `opcache.optimization_level=0` |
| JIT tracing (`opcache.jit=tracing`, `on`, `1`, or a `CRTO` number with trigger T 3 or 5) | no | `opcache.jit=disable` |
| JIT function (`opcache.jit=function`, or a `CRTO` number with trigger T 0, 1 or 2) | no | `opcache.jit=disable` |
| File cache (`opcache.file_cache`, `opcache.file_cache_only`) | no | `opcache.file_cache=` |
| Preload (`opcache.preload`) | no | `opcache.preload=` |
| `opcache.protect_memory=1` | no | `opcache.protect_memory=0` |
| OPcache under FPM, CGI or another server SAPI | no | `opcache.enable=0`, or the CLI |

The JIT counts as on only with `opcache.jit_buffer_size` above 0. phpdbg counts
as the CLI. PHP 8.5 and later build OPcache in; on 8.4 it is `opcache.so`.
