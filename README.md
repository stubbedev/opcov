# opcov

Opcode-level line, branch-outcome and loop coverage for PHP, with per-test
attribution, as a PHP extension written in Rust.

opcov never rewrites source. When PHP compiles a file you selected, opcov
builds the engine's own control-flow graph of every function and puts
**fire-once probes** on the opcodes that start a line, decide a branch or run a
loop. A probe records what happened and puts the opcode's real handler back, so
code that has been seen runs at full speed until the next test re-arms it.
Files outside the selection keep their real handlers and run exactly as
without opcov.

What it reports per function:

- **lines**, following PCOV's executable-line rules;
- **every branch outcome**, each with its kind: `if`/`elseif` true and false
  (including an implicit `else`), `&&`/`||`/`and`/`or` short-circuit and
  evaluate, both sides of `??`, `??=`, `?:` and `?->`, every `match` arm and
  "no arm", every `switch` case and default, each `catch`, and `finally`
  entered normally or by an exception;
- **loops** that never ran zero times and loops that never ran two or more
  times in one activation;
- **which tests** hit each line, outcome and loop finding.

[docs/branch-semantics.md](docs/branch-semantics.md) defines every outcome;
[docs/how-it-works.md](docs/how-it-works.md) explains the probes.

## Why

A changed-code coverage gate needs branch coverage of every outcome, per test,
fast. The existing tools fall short:

| Tool | Approach | Gap |
|---|---|---|
| Xdebug branch/path coverage | hook on every opcode | slow; does not split `??`, `?:`, `?->`; no loop iteration counts |
| PCOV | hook on every opcode | lines only |
| [turbocov](https://github.com/shipmonk-rnd/turbocov) | fire-once handler probes | lines only; opcov builds on its mechanism |
| [fast-xdebug](https://github.com/juslintek/fast-xdebug) | C, block-based | proof of concept |
| [sonda](https://github.com/danie1net0/sonda) | source instrumentation | rewrites source |

## Status

opcov is pre-1.0 (version 0.1.0, unreleased). Before 1.0.0 a minor release may
break compatibility ([docs/support.md](docs/support.md)). What works today:

- the extension on PHP 8.4, 8.5 and 8.6, NTS and ZTS, on Linux (glibc, musl)
  and macOS arm64, on the HYBRID, CALL and TAILCALL VMs;
- line, branch-outcome and loop collection with per-test attribution, and the
  canonical report `opcov-report/1`;
- the PHPUnit 12/13 and Pest 4 integration, including paratest, and a
  php-code-coverage driver for PHPUnit 13.3+ with line and branch coverage;
- `opcov merge`, `convert` (lcov, Clover, Cobertura, HTML), `gate` and
  `doctor`;
- OPcache under the CLI; every unsupported setup (JIT, file cache, preload,
  optimizer, shared SAPIs, Xdebug coverage, PCOV) is refused with the setting to
  change ([docs/troubleshooting.md](docs/troubleshooting.md)).

## Install

Every channel installs a prebuilt extension; none needs Rust. Pick the
artifact that matches your PHP's minor version and thread safety (`php -v`).
The full instructions, with checksums and attestations, are in
[docs/install.md](docs/install.md).

```sh
pie install opcov/opcov
```

```dockerfile
FROM php:8.5-cli
COPY --from=ghcr.io/stubbedev/opcov:0.1.0-php8.5 / /
```

```sh
nix shell github:stubbedev/opcov#php85-with-opcov
```

Loading the extension changes nothing until `opcov.enabled=1`
([docs/configuration.md](docs/configuration.md)).

## Quick start: PHPUnit 12/13 and Pest 4

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

Run the tests through the launcher, which turns `<source>` into the opcov INI
settings for every PHP process of the run:

```sh
vendor/bin/opcov-phpunit vendor/bin/phpunit
vendor/bin/opcov-phpunit vendor/bin/pest
```

The report is `build/opcov.json`. Parallel runs, PHPUnit 13.3's custom coverage
driver and conversion to php-code-coverage's `.cov` are in
[docs/phpunit.md](docs/phpunit.md).

## The `opcov` command

`opcov merge` combines the reports of parallel workers into the report a serial
run would produce, byte for byte. `opcov convert` writes lcov, Clover,
Cobertura or HTML. `opcov gate` checks the functions a diff touches for
uncovered lines, unseen outcomes and loops never run zero or two or more times,
and lists the tests that ran each line. `opcov doctor` lists outcomes that
match no source construct. See [docs/cli.md](docs/cli.md).

```sh
opcov merge build/opcov.*.json -o build/opcov.json
git diff -U0 origin/main... | opcov gate --report build/opcov.json --diff -
```

## Benchmarks

Instructions executed with coverage relative to the same run without
coverage (cycles in parentheses), median of 5 runs, PHP 8.5.13 on an Intel
Core i5-10400F desktop, load average 3.09 at the start of the run
([docs/benchmarks.md](docs/benchmarks.md) has the full tables with wall time,
allocations and memory, the method and profiles). opcov collects lines,
branch outcomes and loops with per-test attribution; PCOV collects lines only.

| Workload | opcov | PCOV | Xdebug lines | Xdebug branch/path |
|---|---|---|---|---|
| Laravel feature tests (spatie/laravel-query-builder, 367 tests) | **1.02x** (1.01x) | 2.79x (2.16x) | 8.30x (5.77x) | over 15 min, timed out (no coverage: 4.4 s) |
| Carbon unit suite (6,349 tests) | **1.56x** (1.42x) | 2.38x (2.09x) | 22.09x (14.92x) | over 15 min, timed out (no coverage: 45 s) |
| branch-heavy loop, 400k calls | **1.99x** (1.89x) | 3.60x (3.11x) | 17.25x (16.48x) | over 15 min, timed out (no coverage: 0.29 s) |
| 20,000 tiny tests | **6.61x** (4.47x) | 3.70x (2.34x) | 51.04x (42.72x) | 135.52x (308.95x) |

On 20,000 tiny tests opcov's per-test cost (85 k instructions, the largest
single part of it building the array `stop()` returns) is higher than PCOV's
(41 k); the profile is in [docs/benchmarks.md](docs/benchmarks.md#profiles).

Gating a real branch (symfony/string v8.1.0 to v8.1.10, 2,932 tests): the
PHPUnit run that produces the gate's coverage takes 314 times as long under
Xdebug path coverage (370.8 s) as under opcov (1.2 s), and opcov's run costs
1.16x the run without coverage (median of 3 runs, load average 2.32 at the
start of the collection; [docs/gate.md](docs/gate.md#results)).

## Documentation

[docs/README.md](docs/README.md) indexes every page:

| Page | Contents |
|---|---|
| [install.md](docs/install.md) | Every install channel |
| [configuration.md](docs/configuration.md) | INI settings and the `\opcov\` API |
| [phpunit.md](docs/phpunit.md) | PHPUnit and Pest integration |
| [cli.md](docs/cli.md) | `opcov merge`, `convert`, `gate`, `doctor` |
| [report-schema.md](docs/report-schema.md) | The canonical report and how to consume it |
| [branch-semantics.md](docs/branch-semantics.md) | Every outcome kind and loop finding, and the differences from Xdebug and PCOV |
| [how-it-works.md](docs/how-it-works.md) | The probe mechanism and its limits |
| [troubleshooting.md](docs/troubleshooting.md) | Every refusal message and its fix |
| [support.md](docs/support.md) | Versioning and supported PHP versions and platforms |
| [compatibility.md](docs/compatibility.md) | OPcache, JIT, other extensions, threads and processes |
| [parity.md](docs/parity.md) | The differential suite against PCOV and Xdebug |
| [benchmarks.md](docs/benchmarks.md) | The benchmark harness |
| [contributing.md](docs/contributing.md) | Building, testing, adding a PHP version |

## Development

Build from source, the dev shells, every test layer, the lints, the Nix
outputs, CI and the commit conventions are in
[docs/contributing.md](docs/contributing.md). In short:

```sh
nix develop
just test
just phpt
```

## License

MIT; see [LICENSE](LICENSE).
