# opcov

Opcode-level line, branch-outcome and loop coverage for PHP, with per-test
attribution, as a PHP extension written in Rust.

[![CI](https://github.com/stubbedev/opcov/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/stubbedev/opcov/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/stubbedev/opcov)](https://github.com/stubbedev/opcov/releases)
[![Packagist](https://img.shields.io/packagist/v/opcov/phpunit?label=opcov%2Fphpunit)](https://packagist.org/packages/opcov/phpunit)
[![License](https://img.shields.io/github/license/stubbedev/opcov)](LICENSE)

## What it measures

- **Lines**, following PCOV's executable-line rules.
- **Every branch outcome**, each with its kind: `if`/`elseif` true and false
  (including an implicit `else`), `&&`/`||`/`and`/`or`, both sides of `??`,
  `??=`, `?:` and `?->`, every `match` arm and "no arm", every `switch` case,
  each `catch`, and `finally` entered normally or by an exception.
- **Loops** that never ran zero times and loops that never ran two or more
  times in one activation.
- **Which tests** hit each line, outcome and loop finding.

opcov never rewrites source. It builds the engine's control-flow graph of
every selected function and puts fire-once probes on the opcodes that start a
line, decide a branch or run a loop; a probe records what happened and puts the
real handler back, so seen code runs at full speed. Unselected files run
exactly as without opcov. See [how-it-works.md](docs/how-it-works.md) and
[branch-semantics.md](docs/branch-semantics.md).

## Performance

Instructions executed relative to the same run without coverage, median of 3
runs, PHP 8.5.13. opcov collects lines, outcomes and loops; PCOV lines only.

| Workload | opcov | PCOV | Xdebug lines | Xdebug branch/path |
|---|---|---|---|---|
| Laravel package feature tests (367 tests) | **1.01x** | 2.79x | 8.30x | timed out (>15 min) |
| Carbon unit suite (6,349 tests) | **1.48x** | 2.38x | 22.09x | timed out (>15 min) |
| Branch-heavy loop, 400k calls | **1.84x** | 3.60x | 17.25x | timed out (>15 min) |
| 20,000 tiny tests | **3.68x** | 3.70x | 51.03x | 135.50x |

Gating a real branch (symfony/string, 2,932 tests), the coverage run takes 314
times as long under Xdebug path coverage as under opcov
([gate.md](docs/gate.md)). On a 4.97-million-line corpus (Symfony, Laravel,
Drupal, Doctrine, PHPUnit) 99.94% of source outcomes map to a source construct
with an exact span ([scale.md](docs/scale.md),
[branch-semantics.md](docs/branch-semantics.md)). Full tables, method and
profiles: [benchmarks.md](docs/benchmarks.md).

## Install

Every channel installs a prebuilt extension; none needs Rust. Pick the
artifact matching your PHP minor version and thread safety (`php -v`).

```sh
pie install opcov/opcov
```

```dockerfile
FROM php:8.5-cli
COPY --from=ghcr.io/stubbedev/opcov:0.1.3-php8.5 / /
```

```sh
nix shell github:stubbedev/opcov#php85-with-opcov
```

Supported: PHP 8.4, 8.5 and 8.6, NTS and ZTS; Linux glibc and musl on x86_64
and arm64; macOS arm64. Release archives with checksums and attestations, and
every other channel, are in [install.md](docs/install.md). Loading the
extension changes nothing until `opcov.enabled=1`.

## Quick start: PHPUnit 12/13 and Pest

```sh
composer require --dev opcov/phpunit
```

Register the extension in `phpunit.xml` (Pest reads the same file):

```xml
<phpunit bootstrap="vendor/autoload.php">
    <source>
        <include>
            <directory>src</directory>
        </include>
    </source>
    <extensions>
        <bootstrap class="OpCov\PHPUnit\Extension">
            <parameter name="output" value="build/opcov.json"/>
        </bootstrap>
    </extensions>
</phpunit>
```

Run the tests through the launcher, which turns `<source>` into opcov's INI
settings for every PHP process of the run, paratest workers included:

```sh
vendor/bin/opcov-phpunit vendor/bin/phpunit
vendor/bin/opcov-phpunit vendor/bin/pest --parallel
```

Convert the report for other tools, or gate the changed code of a branch:

```sh
opcov convert build/opcov.json --format lcov -o coverage.lcov
opcov convert build/opcov.json --format clover -o clover.xml
git diff -U0 origin/main... | opcov gate --report build/opcov.json --diff -
```

`opcov gate` reports uncovered lines, unseen outcomes and loops never run zero
or two or more times in the functions the diff touches. `opcov impact`
chooses the tests a change can reach from a baseline report, so the gate needs
a run of those tests only ([gate.md](docs/gate.md#running-only-the-tests-the-change-can-reach)), and
`opcov stitch` lays that run over the baseline for a report of the whole
branch. The PHPUnit 13.3
coverage driver, `.cov` conversion and `opcov merge`/`doctor` are in
[phpunit.md](docs/phpunit.md) and [cli.md](docs/cli.md).

## Comparison

| | opcov | PCOV | Xdebug | [turbocov](https://github.com/shipmonk-rnd/turbocov) |
|---|---|---|---|---|
| Lines | yes | yes | yes | yes |
| Branch outcomes | every outcome, named by kind | no | branch/path coverage | no |
| Both sides of `??`, `?:`, `?->` | yes | no | no | no |
| Loops run zero / two or more times | yes | no | no | no |
| Own report with per-test attribution | yes | no (via php-code-coverage) | no (via php-code-coverage) | no |
| Carbon suite overhead (instructions) | 1.48x | 2.38x | 22.09x lines; branch timed out | 1.02x |

opcov builds on turbocov's probe mechanism. Every difference from Xdebug and
PCOV is listed in [branch-semantics.md](docs/branch-semantics.md).

## Documentation

[docs/README.md](docs/README.md) indexes every page. The most used:

- [install.md](docs/install.md): every install channel and platform requirement
- [configuration.md](docs/configuration.md): INI settings and the `\opcov\` API
- [phpunit.md](docs/phpunit.md): PHPUnit, Pest, paratest and the 13.3 driver
- [cli.md](docs/cli.md): `opcov merge`, `convert`, `gate`, `doctor`
- [gate.md](docs/gate.md): gating changed code in CI
- [troubleshooting.md](docs/troubleshooting.md): every refusal message and its fix
- [support.md](docs/support.md): versioning and supported PHP versions
- [CHANGELOG.md](CHANGELOG.md): release notes

opcov is pre-1.0: before 1.0.0 a minor release may break compatibility.

## Development

```sh
nix develop
just check
```

`just check` runs formatting, lints, dependency checks, the Rust tests, the
build and the `.phpt` suite. Dev shells per PHP version, sanitizers, the
parity suite and commit conventions are in
[contributing.md](docs/contributing.md).

## License

MIT; see [LICENSE](LICENSE).
