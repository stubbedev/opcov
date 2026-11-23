# Benchmarks

The benchmark harness runs the same PHP workloads under every coverage driver
and reports wall time (median of 10 runs, with min and max), peak RSS and the
size of the data each driver hands back. It shares the driver matrix, the PHP
provisioning and the PHPUnit integration with the parity suite
(`docs/parity.md`).

## Running it

```sh
tools/measure/bench                                        # every workload, every driver that builds, 10 runs
tools/measure/bench --workload branchy --runs 3            # one workload
tools/measure/bench --driver none --driver opcov --check   # regression check against bench/baseline.json
tools/measure/bench --write-docs                           # rewrite this page's tables for this PHP
tools/measure/bench --update-baseline                      # store opcov's overhead as the new baseline
OPCOV_MEASURE_PHP=php84 tools/measure/bench
```

Options: `--workload` and `--driver` (repeatable), `--runs <n>` (default 10),
`--budget <seconds>` per run (default 900; a driver that exceeds it is shown as
timed out and skipped for the rest of that workload), `--check`,
`--update-baseline`, `--write-docs`. Raw results go to
`target/measure-cache/bench/<php>/results.json`.

## Method

- **Pinning.** The issue asked for a pinned Docker image; the harness pins with
  Nix instead. `tools/measure/env.nix` takes PHP, PCOV, Xdebug, composer, GNU
  time and perf from the nixpkgs revision locked in `flake.lock` and builds
  turbocov 0.1.1 from its tagged source with a fixed hash, so a run is
  reproducible from the repository alone, without Docker. The corpus suites
  are pinned by commit and stored composer lock (`tools/measure/corpus/`).
- **Drivers.** `none` (no coverage extension loaded), opcov (lines, branch
  outcomes and loops: the extension has no lines-only mode), PCOV, Xdebug
  lines (`XDEBUG_CC_UNUSED | XDEBUG_CC_DEAD_CODE`), Xdebug branch (plus
  `XDEBUG_CC_BRANCH_CHECK`), turbocov. turbocov and PCOV do not build for
  PHP 8.6 (nor Xdebug), so 8.6 measures `none` and opcov only. Every process loads at most one coverage extension,
  OPcache is off, and each driver is limited to the measured source directory.
- **Calls.** Each driver is called the way its PHPUnit driver calls it, once
  per test (opcov `start`/`stop($testId)`/`reset`, PCOV
  `start`/`stop`/`waiting`/`collect`/`clear`, Xdebug
  `start`/`get`/`stop`, turbocov `start`/`stop`). The data is dropped in the
  timed runs; an extra untimed run measures its size (the serialized length of
  everything `stop()` returned, summed over the tests, the "payload" column).
  The same untimed run has opcov write its canonical report with
  `\opcov\write_report()` (the "report" column); the other drivers have no
  report of their own (php-code-coverage writes theirs), so theirs is `n/a`.
  The `none` runs load the same PHPUnit extension and make no driver calls, so
  the event overhead is the same everywhere.
- **Timing.** Wall time of the whole PHP process, measured around a blocking
  `proc_close`; peak RSS from GNU time's `--verbose` report. Rounds are
  interleaved (every driver once per round) so that drift in clock speed or
  background load spreads over all drivers.
- **Machine state.** CPU model, governor, turbo state and the load average at
  the start are recorded with every result. The numbers below come from a
  developer machine with turbo on and other work running, not from an isolated
  runner; read min and max, not only the median.

## Workloads

| Workload | What it stresses |
|---|---|
| `branchy` | steady-state cost: one collection around 400k calls of a function full of branches; every probe has fired after the first call |
| `tiny-tests` | per-test cost: 20,000 tiny tests over the same class, each with start, stop and the per-test reset |
| `carbon` | a large unit suite: Carbon 3.15.1, 6,349 tests, `src/Carbon` measured |
| `laravel-query-builder` | framework-heavy feature tests: spatie/laravel-query-builder 7.3.9 on Laravel 12 through Orchestra Testbench, in-memory SQLite, Pest |

## Results

Not measured yet: the engine was still changing when the harness was
finished. `tools/measure/bench --write-docs` fills this section.

<!-- bench:php85:begin -->
<!-- bench:php85:end -->

## Regression check

`tools/measure/bench --driver none --driver opcov --check` fails (exit 1) when,
on any workload, opcov's time divided by the no-coverage time of the same run
exceeds the ratio stored in `bench/baseline.json` for that PHP version by more
than 10%. The ratio, not the absolute time, is compared, so the baseline holds
across machines of different speed as long as the relative cost of opcov's
probes does not change. Record a new baseline with `--update-baseline` after
an intended change and commit `bench/baseline.json`.

## CI

The regression check needs a quiet machine: on a shared CI runner the noise
between runs is larger than 10%. Run it nightly or on a dedicated runner:

```sh
OPCOV_MEASURE_PHP=php85 OPCOV_EXTENSION="$(nix build --print-out-paths .#opcov-php85)/lib/php/extensions/opcov.so" \
    tools/measure/bench --driver none --driver opcov --check
```

The full matrix (all drivers, `--write-docs`) takes over an hour, mostly Xdebug
on Carbon, and is meant for refreshing this page, not for every push.
