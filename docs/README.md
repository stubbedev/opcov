# opcov documentation

## Using opcov

- [install.md](install.md): every install channel (PIE, release archives, Docker, Nix, from source) and the platform requirements.
- [configuration.md](configuration.md): the `opcov.*` INI settings, file selection, executable-line rules and the `\opcov\` PHP API.
- [phpunit.md](phpunit.md): the `opcov/phpunit` package for PHPUnit 12/13 and Pest 4, the launcher, paratest, the PHPUnit 13.3 driver and `.cov` conversion.
- [cli.md](cli.md): the `opcov` command: `merge`, `convert`, `gate` and `doctor`, with exit codes and output formats.
- [report-schema.md](report-schema.md): the canonical report `opcov-report/1`, its version policy, how to consume it and how merging works.
- [branch-semantics.md](branch-semantics.md): every outcome kind and loop finding, what counts toward a verdict, and the differences from Xdebug and PCOV.
- [troubleshooting.md](troubleshooting.md): every message opcov refuses collection with, and the fix.
- [support.md](support.md): versioning, supported PHP versions and platforms, release notes.
- [compatibility.md](compatibility.md): OPcache and JIT, other extensions, threads, processes and language features.

## How opcov is built

- [how-it-works.md](how-it-works.md): the probe mechanism (compile hook, CFG, probe label, outcome decision, loops, units, reset) and its limits.
- [parity.md](parity.md): the differential suite against PCOV and Xdebug and its allowlist.
- [benchmarks.md](benchmarks.md): the benchmark harness, its method and results.
- [contributing.md](contributing.md): building, the dev shells, every test layer, adding a PHP version and the commit conventions.

## Decision records

- [ADR 0001](adr/0001-bindings.md): Zend engine bindings, the C shim, panic handling and every engine symbol used.
- [ADR 0002](adr/0002-outcome-probes.md): branch-outcome and loop probes: evaluate in the probe or watch the successor.
- [ADR 0003](adr/0003-declarations-only-files.md): files that only declare have no executable line.
