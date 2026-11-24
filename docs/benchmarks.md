# Benchmarks

The benchmark harness runs the same PHP workloads under every coverage driver
and reports, per driver and workload, hardware counters (instructions, the
primary metric, cycles, branches and branch misses, cache misses, page faults,
task clock), allocation counts, peak memory, the size of the data each driver
hands back and, as a secondary column, wall time (median of the runs, with
min and max). It shares the driver matrix, the PHP
provisioning and the PHPUnit integration with the parity suite
(`docs/parity.md`).

## Running it

```sh
tools/measure/bench                                        # every workload, every driver that builds, 3 runs
tools/measure/bench --workload branchy --runs 3            # one workload
tools/measure/bench --driver none --driver opcov --check   # regression check against bench/baseline.json
tools/measure/bench --write-docs                           # rewrite this page's tables for this PHP
tools/measure/bench --update-baseline                      # store opcov's overhead as the new baseline
tools/measure/bench report --write-docs --update-baseline  # the same outputs from the recorded samples, without measuring
OPCOV_MEASURE_PHP=php84 tools/measure/bench
```

Options: `--workload` and `--driver` (repeatable), `--runs <n>` (default 3),
`--timeout-minutes <n>` per driver run (default 15; a driver that exceeds it
is shown as "> n min" and skipped for the rest of that workload), `--fresh`,
`--check`, `--update-baseline`, `--write-docs`.

Every measurement (a driver's payload run, each timed round, the
`USE_ZEND_ALLOC=0` run, or a timeout) is appended to
`target/measure-cache/bench/<php>/samples.jsonl` as soon as it finishes, one
JSON object per line, keyed by workload, driver, step and round and tagged
with the build it measured: the PHP release, the measurement environment's
Nix store path and, for opcov, the SHA-256 of the extension. A run skips every
sample already recorded for the same build, so an interrupted or restarted
run resumes where it stopped and loses at most the run in progress. A recorded
timeout is kept only while `--timeout-minutes` is not larger than the limit it
hit; raising the limit measures that driver again. `--fresh` discards the
recorded samples of the selected workloads and drivers for this build and
measures them again. Rebuilding opcov changes its hash, so only opcov is
measured again.

`tools/measure/bench report` takes the same `--workload`, `--driver` and
`--runs` selection and the `--write-docs`, `--update-baseline` and `--check`
outputs, and works from the recorded samples alone. It fails, naming the
missing sample and the command that measures it, when a selected workload,
driver or round has not been recorded for this build. The machine description
in the tables is the one recorded with the earliest sample the report uses.

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
- **Counters.** Every timed run is wrapped as `timeout perf stat -x, -e
  instructions,cycles,branches,branch-misses,cache-misses,page-faults,task-clock
  -- time --verbose env LD_PRELOAD=... php ...`. perf counts the process tree
  under it (PHP and the few instructions of GNU time's own start). The table
  header names the events as perf reported them: with
  `kernel.perf_event_paranoid` at 2 (recorded with the machine) perf counts
  user space only (`cycles:u`); with a lower setting it counts kernel time
  too. An event perf cannot count is recorded as unavailable instead of
  failing the run. Instructions do not depend on clock speed, turbo or other
  load, so they are the primary metric and the regression check uses them;
  cycles depend on the cache and branch predictor state the machine leaves
  behind and vary more.
- **Allocations.** `tools/measure/alloc/count.c`, built into the measurement
  environment, is preloaded into PHP. It counts calls of `malloc`, `calloc`,
  `realloc`, `memalign`/`aligned_alloc`/`posix_memalign`/`valloc` (summed as
  "allocation calls"), `free`, and the bytes requested, with atomic counters,
  and each process writes its counts to `<run>.alloc.<pid>` at exit. A forked
  child starts from zero, so nothing is counted twice, and the harness sums
  the files of the process tree. PHP's own allocator (Zend MM) takes memory in
  2 MiB chunks with `mmap` and serves `emalloc` from them, so in the timed runs
  ("Zend MM" columns) the counts cover what bypasses Zend MM: the extensions'
  own heaps (opcov's Rust allocations, PCOV's and Xdebug's `malloc`s), libc
  and PHP's persistent allocations. One extra run per driver with
  `USE_ZEND_ALLOC=0` ("USE_ZEND_ALLOC=0" columns) sends every PHP allocation
  through `malloc` and so counts per-object allocations; it is not timed.
  "PHP peak" is `memory_get_peak_usage(true)` read inside PHP at the end of
  the timed run (the Zend MM chunks in use at the peak).
- **Timing.** Wall time of the whole PHP process, measured around a blocking
  `proc_close`, is kept as a secondary column; peak RSS comes from GNU time's
  `--verbose` report. Rounds are interleaved (every driver once per round) so
  that drift in clock speed or background load spreads over all drivers. The
  counting shim and perf stay in place for every driver, including `none`, so
  their small cost is the same everywhere. Three rounds are enough for the
  instruction counts: in the run below the five runs of a driver agree to
  within 0.01%, except on Carbon (up to 3.4%, its tests depend on the clock).
- **Machine state.** CPU model, governor, turbo state and the load average at
  the start are recorded with every result. The numbers below come from a
  developer machine with turbo on and other work running, not from an isolated
  runner; read min and max, not only the median.

## Workloads

| Workload | What it stresses |
|---|---|
| `branchy` | steady-state cost: one collection around 400k calls of a function full of branches. Line probes are gone after the first call, but four branch sources never see all their outcomes (the fallbacks of two `??` that always find a value, and the `foreach` that never runs zero times), so they stay armed and fire on every call |
| `tiny-tests` | per-test cost: 20,000 tiny tests over the same class, each with start, stop and the per-test reset |
| `carbon` | a large unit suite: Carbon 3.15.1, 6,349 tests, `src/Carbon` measured |
| `laravel-query-builder` | framework-heavy feature tests: spatie/laravel-query-builder 7.3.9 on Laravel 12 through Orchestra Testbench, in-memory SQLite, Pest |

## Results

Measured on 2026-11-24 with a release build of opcov from commit 6d4f418
(`tools/measure/bench --update-baseline --write-docs`, all four workloads,
every driver, 5 interleaved rounds, about 3 h). Machine: Intel Core i5-10400F,
6 cores and 12 threads, `powersave` governor with turbo on (intel_pstate),
Linux 6.18.64, 31.3 GiB RAM, PHP 8.5.13 NTS from the locked nixpkgs,
`kernel.perf_event_paranoid` 2, so every counter is user space only. It is a
developer desktop that was busy with other work during the run: the load
average was 3.09 (5.42 over 5 minutes, 7.95 over 15) at the start and between
1 and 7 during it. Instruction counts are unaffected by that load: the five
runs of a driver agree to within 0.01% on branchy, tiny-tests and the
Laravel suite and within 3.4% on Carbon, whose tests depend on the clock.
Cycles and wall time are not: their spread between runs reaches 84% (min and
max are in the tables), so read their medians as approximate. A
`--check` on branchy and tiny-tests right after the run passed on
instructions and failed on cycles ([Regression check](#regression-check)).

<!-- bench:php85:begin -->
PHP 8.5.13, median of 5 runs. Machine: cpu Intel(R) Core(TM) i5-10400F CPU @ 2.90GHz, logical cpus 12, governor powersave, turbo on (intel_pstate), kernel Linux 6.18.64, memory 31.3 GiB, perf_event_paranoid 2, load average at start 3.09 5.42 7.95.

Drivers: PHP 8.5.13, opcov 0.1.0, pcov 1.0.12, xdebug 3.5.3, xdebug 3.5.3, turbocov 0.1.1.

Counters: `instructions:u`, `cycles:u`, `branches:u`, `branch-misses:u`, `cache-misses:u`, `page-faults:u`, `task-clock:u`, as `perf stat` reported them.

#### branchy

Synthetic branch-heavy CPU loop: one collection around 400k calls of a function with if/elseif, &&/||, ??, ?:, match, switch, foreach and ?->; 2 tests.

| driver | instructions | x none | cycles | x none | branch misses | cache misses | page faults | task-clock s | wall s (min-max) | x none | peak RSS MiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| no coverage | 2.374 G | 1.00 | 1.060 G | 1.00 | 2.531 M | 576.087 k | 3.386 k | 0.275 | 0.291 (0.289-0.327) | 1.00 | 37.3 |
| opcov (lines, outcomes, loops) | 4.724 G | 1.99 | 2.004 G | 1.89 | 3.972 M | 682.656 k | 3.437 k | 0.508 | 0.524 (0.506-0.619) | 1.80 | 38.5 |
| PCOV | 8.545 G | 3.60 | 3.296 G | 3.11 | 8.362 M | 873.870 k | 3.398 k | 0.837 | 0.852 (0.795-0.871) | 2.93 | 37.2 |
| Xdebug (lines) | 40.946 G | 17.25 | 17.467 G | 16.48 | 45.873 M | 3.401 M | 3.530 k | 4.376 | 4.393 (4.215-4.456) | 15.11 | 38.0 |
| Xdebug (branch) | timed out after 900 s |  |  |  |  |  |  |  |  |  |  |
| turbocov | 2.402 G | 1.01 | 1.079 G | 1.02 | 2.689 M | 608.265 k | 3.400 k | 0.280 | 0.296 (0.290-0.417) | 1.02 | 37.5 |

| driver | allocation calls (Zend MM) | x none | bytes allocated (Zend MM) | allocation calls (USE_ZEND_ALLOC=0) | x none | bytes allocated (USE_ZEND_ALLOC=0) | PHP peak MiB | payload MiB | report MiB |
|---|---|---|---|---|---|---|---|---|---|
| no coverage | 40.380 k | 1.00 | 5.9 MiB | 4.578 M | 1.00 | 565.8 MiB | 2.0 | 0.00 | n/a |
| opcov (lines, outcomes, loops) | 1.641 M | 40.64 | 152.5 MiB | 6.180 M | 1.35 | 712.7 MiB | 2.0 | 0.00 | 0.00 |
| PCOV | 40.455 k | 1.00 | 5.9 MiB | 4.578 M | 1.00 | 567.0 MiB | 2.0 | 0.00 | n/a |
| Xdebug (lines) | 848.339 k | 21.01 | 18.9 MiB | 46.358 M | 10.13 | 5853.9 MiB | 2.0 | 0.00 | n/a |
| Xdebug (branch) | timed out after 900 s |  |  |  |  |  |  |  |  |
| turbocov | 40.472 k | 1.00 | 5.9 MiB | 4.578 M | 1.00 | 566.0 MiB | 2.0 | 0.00 | n/a |

#### tiny-tests

20,000 tiny tests touching the same class, each bracketed by start/stop and the driver's per-test reset (opcov `reset()`, PCOV `clear()`); 20001 tests.

| driver | instructions | x none | cycles | x none | branch misses | cache misses | page faults | task-clock s | wall s (min-max) | x none | peak RSS MiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| no coverage | 303.140 M | 1.00 | 208.571 M | 1.00 | 648.146 k | 548.605 k | 3.461 k | 0.063 | 0.080 (0.064-0.106) | 1.00 | 37.5 |
| opcov (lines, outcomes, loops) | 2.005 G | 6.61 | 932.034 M | 4.47 | 2.198 M | 765.723 k | 6.836 k | 0.248 | 0.266 (0.243-0.462) | 3.32 | 51.1 |
| PCOV | 1.123 G | 3.70 | 488.185 M | 2.34 | 1.384 M | 614.864 k | 3.474 k | 0.132 | 0.149 (0.146-0.259) | 1.86 | 37.8 |
| Xdebug (lines) | 15.472 G | 51.04 | 8.909 G | 42.72 | 22.131 M | 1.218 M | 3.894 k | 2.252 | 2.267 (2.139-3.471) | 28.25 | 39.7 |
| Xdebug (branch) | 41.082 G | 135.52 | 64.439 G | 308.95 | 216.076 M | 856.321 M | 24.447 k | 16.158 | 16.185 (15.177-16.372) | 201.69 | 119.4 |
| turbocov | 477.105 M | 1.57 | 222.348 M | 1.07 | 666.022 k | 581.618 k | 3.473 k | 0.066 | 0.081 (0.081-0.127) | 1.02 | 37.6 |

| driver | allocation calls (Zend MM) | x none | bytes allocated (Zend MM) | allocation calls (USE_ZEND_ALLOC=0) | x none | bytes allocated (USE_ZEND_ALLOC=0) | PHP peak MiB | payload MiB | report MiB |
|---|---|---|---|---|---|---|---|---|---|
| no coverage | 40.380 k | 1.00 | 5.9 MiB | 264.794 k | 1.00 | 30.3 MiB | 2.0 | 0.04 | n/a |
| opcov (lines, outcomes, loops) | 741.128 k | 18.35 | 84.2 MiB | 2.406 M | 9.09 | 281.2 MiB | 2.0 | 17.26 | 4.29 |
| PCOV | 40.455 k | 1.00 | 5.9 MiB | 464.955 k | 1.76 | 1457.2 MiB | 4.0 | 8.39 | n/a |
| Xdebug (lines) | 31.347 M | 776.30 | 1119.0 MiB | 34.572 M | 130.56 | 1602.7 MiB | 2.0 | 18.30 | n/a |
| Xdebug (branch) | 62.499 M | 1547.76 | 5584.7 MiB | 86.514 M | 326.72 | 7592.2 MiB | 4.0 | 208.36 | n/a |
| turbocov | 40.486 k | 1.00 | 5.9 MiB | 385.036 k | 1.45 | 81.7 MiB | 2.0 | 5.72 | n/a |

#### carbon

Carbon's PHPUnit suite (large unit suite, many small tests, `src/Carbon` measured); 6349 tests.

| driver | instructions | x none | cycles | x none | branch misses | cache misses | page faults | task-clock s | wall s (min-max) | x none | peak RSS MiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| no coverage | 225.036 G | 1.00 | 173.296 G | 1.00 | 1.285 G | 338.939 M | 140.649 k | 44.060 | 45.445 (41.952-59.410) | 1.00 | 190.6 |
| opcov (lines, outcomes, loops) | 351.563 G | 1.56 | 246.820 G | 1.42 | 1.828 G | 311.767 M | 160.919 k | 61.612 | 62.699 (61.414-108.726) | 1.38 | 270.8 |
| PCOV | 535.605 G | 2.38 | 362.687 G | 2.09 | 3.537 G | 552.765 M | 142.921 k | 90.180 | 91.294 (87.199-116.689) | 2.01 | 196.9 |
| Xdebug (lines) | 4971.312 G | 22.09 | 2586.369 G | 14.92 | 9.883 G | 4.767 G | 145.634 k | 646.305 | 648.222 (583.317-720.358) | 14.26 | 207.6 |
| Xdebug (branch) | timed out after 900 s |  |  |  |  |  |  |  |  |  |  |
| turbocov | 230.553 G | 1.02 | 165.376 G | 0.95 | 1.272 G | 273.215 M | 141.839 k | 42.037 | 43.122 (42.027-71.528) | 0.95 | 195.2 |

| driver | allocation calls (Zend MM) | x none | bytes allocated (Zend MM) | allocation calls (USE_ZEND_ALLOC=0) | x none | bytes allocated (USE_ZEND_ALLOC=0) | PHP peak MiB | payload MiB | report MiB |
|---|---|---|---|---|---|---|---|---|---|
| no coverage | 355.888 k | 1.00 | 160.3 MiB | 189.422 M | 1.00 | 33106.1 MiB | 148.5 | 0.01 | n/a |
| opcov (lines, outcomes, loops) | 26.207 M | 73.64 | 2849.0 MiB | 222.133 M | 1.17 | 39094.7 MiB | 148.5 | 130.95 | refused |
| PCOV | 355.962 k | 1.00 | 160.3 MiB | 190.153 M | 1.00 | 36152.3 MiB | 154.5 | 243.22 | n/a |
| Xdebug (lines) | 455.224 M | 1279.12 | 21824.8 MiB | timed out after 900 s | n/a |  | 150.5 | 220.19 | n/a |
| Xdebug (branch) | timed out after 900 s |  |  |  |  |  |  |  |  |
| turbocov | 473.169 k | 1.33 | 204.2 MiB | 190.149 M | 1.00 | 35947.8 MiB | 148.5 | 60.74 | n/a |

opcov (lines, outcomes, loops) refused to write the report: opcov\write_report(): cannot build the report for opcov-payload.json.report.json: src/Carbon/Traits/Mixin.php(127) : eval()'d code was compiled from different sources in this process, so no single sha256 describes its coverage

#### laravel-query-builder

Spatie/laravel-query-builder's Pest suite: Laravel feature tests through Orchestra Testbench on in-memory SQLite (`src` measured); 367 tests.

| driver | instructions | x none | cycles | x none | branch misses | cache misses | page faults | task-clock s | wall s (min-max) | x none | peak RSS MiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| no coverage | 20.826 G | 1.00 | 17.187 G | 1.00 | 153.780 M | 94.254 M | 19.792 k | 4.366 | 4.396 (4.324-5.012) | 1.00 | 107.2 |
| opcov (lines, outcomes, loops) | 21.158 G | 1.02 | 17.361 G | 1.01 | 157.166 M | 99.741 M | 20.287 k | 4.462 | 4.479 (4.415-4.890) | 1.02 | 110.1 |
| PCOV | 58.159 G | 2.79 | 37.040 G | 2.16 | 289.519 M | 190.177 M | 19.984 k | 9.423 | 9.442 (8.680-9.992) | 2.15 | 107.2 |
| Xdebug (lines) | 172.906 G | 8.30 | 99.228 G | 5.77 | 505.271 M | 322.379 M | 21.920 k | 24.946 | 24.966 (23.421-25.371) | 5.68 | 114.7 |
| Xdebug (branch) | timed out after 900 s |  |  |  |  |  |  |  |  |  |  |
| turbocov | 21.100 G | 1.01 | 17.910 G | 1.04 | 155.207 M | 117.831 M | 19.851 k | 4.622 | 4.639 (4.358-5.077) | 1.06 | 107.3 |

| driver | allocation calls (Zend MM) | x none | bytes allocated (Zend MM) | allocation calls (USE_ZEND_ALLOC=0) | x none | bytes allocated (USE_ZEND_ALLOC=0) | PHP peak MiB | payload MiB | report MiB |
|---|---|---|---|---|---|---|---|---|---|
| no coverage | 583.039 k | 1.00 | 220.9 MiB | 18.443 M | 1.00 | 3213.2 MiB | 62.5 | 0.00 | n/a |
| opcov (lines, outcomes, loops) | 631.456 k | 1.08 | 229.4 MiB | 18.566 M | 1.01 | 3247.9 MiB | 62.5 | 1.43 | 0.24 |
| PCOV | 583.113 k | 1.00 | 220.9 MiB | 18.456 M | 1.00 | 3289.6 MiB | 62.5 | 1.05 | n/a |
| Xdebug (lines) | 52.695 M | 90.38 | 2333.0 MiB | 96.281 M | 5.22 | 19375.0 MiB | 66.5 | 15.90 | n/a |
| Xdebug (branch) | timed out after 900 s |  |  |  |  |  |  |  |  |
| turbocov | 584.008 k | 1.00 | 221.4 MiB | 18.485 M | 1.00 | 3237.0 MiB | 62.5 | 0.62 | n/a |
<!-- bench:php85:end -->

### Reading the results

Instructions with coverage divided by instructions without, median of 5
runs (cycles in parentheses):

| workload | opcov | PCOV (lines) | turbocov (lines) | Xdebug lines | Xdebug branch/path |
|---|---|---|---|---|---|
| laravel-query-builder | **1.02** (1.01) | 2.79 (2.16) | 1.01 (1.04) | 8.30 (5.77) | over 15 min, timed out |
| carbon | **1.56** (1.42) | 2.38 (2.09) | 1.02 (0.95) | 22.09 (14.92) | over 15 min, timed out |
| branchy | **1.99** (1.89) | 3.60 (3.11) | 1.01 (1.02) | 17.25 (16.48) | over 15 min, timed out |
| tiny-tests | **6.61** (4.47) | 3.70 (2.34) | 1.57 (1.07) | 51.04 (42.72) | 135.52 (308.95) |

opcov collects lines, branch outcomes and loops with per-test attribution;
PCOV and turbocov collect lines only.

- **Framework workload.** On the Laravel feature tests opcov's full
  collection costs 2% more instructions and 1% more cycles than no coverage,
  which meets the epic's target of branch coverage within 1.2x of no
  coverage. PCOV executes 2.79 times the instructions for lines only.
- **Large unit suite.** On Carbon opcov executes 127 G instructions more than
  no coverage over 6,349 tests (1.56x), PCOV 311 G (2.38x), turbocov 6 G
  (1.02x). See the profile below.
- **Xdebug branch/path.** The `xdebug-branch` driver passes
  `XDEBUG_CC_BRANCH_CHECK`, which makes Xdebug record branches and enumerate
  paths. Its first run exceeded the 15-minute budget on branchy, Carbon and
  the Laravel suite, so those rows show "timed out" and it was not run again
  for that workload (the reduced-round rule: a single run over 15 minutes).
  Only tiny-tests finished: 41.1 G instructions and 16.2 s, against opcov's
  2.0 G and 0.27 s. On a real branch with PHPUnit, Xdebug path coverage took
  314 times as long as opcov ([gate.md](gate.md#results)). Xdebug lines on
  Carbon under `USE_ZEND_ALLOC=0` also exceeded the budget, so that cell is
  empty.
- **Allocations.** With Zend MM (the normal setup), opcov makes 1.08x the
  `malloc`-family calls of no coverage on Laravel, 73.6x on Carbon (26.2 M
  calls, 2.8 GiB requested, against 0.36 M), 40.6x on branchy and 18.4x on
  tiny-tests; PCOV and turbocov stay at 1.00 to 1.33x because they allocate
  from Zend MM or not at all per test. opcov's Rust code allocates through
  `malloc`, so every one of those calls is opcov's own. On branchy that is
  1.60 M calls beyond no coverage for 400k calls of `classify()`: exactly 4
  per call, one per resolved successor watch (`Engine::resolve_watch` collects
  the matching successors into a new `Vec` each time a watched successor
  fires, and the `FE_RESET` and three `FE_FETCH` runs per call are watched).
  On tiny-tests it is 35 calls per test, from `stop()` building its per-file
  and per-function vectors, the per-call conflict check (below) and the
  `foreach` watches. Xdebug allocates far more: 776x (lines) and 1,548x
  (branch/path) on tiny-tests. With `USE_ZEND_ALLOC=0`, where PHP's own
  per-object allocations go through `malloc` too, opcov adds 1.01x (Laravel)
  to 1.35x (branchy) on the real workloads.
- **Memory.** opcov's peak RSS is 3% to 42% above no coverage (110 against
  107 MiB on Laravel, 271 against 191 MiB on Carbon), while PHP's own peak
  (`memory_get_peak_usage(true)`) is unchanged: opcov's state lives outside
  Zend MM.
- **Report size.** Carbon has no report size: `\opcov\write_report()` refuses
  the report because `Carbon\Traits\Mixin::loadMixinTrait()` runs `eval()` on
  line 127 with different code for each mixin, so one engine name,
  `Mixin.php(127) : eval()'d code`, stands for several sources and no single
  sha256 describes it. The refusal is documented behaviour
  ([configuration.md](configuration.md)), but it means a Carbon test run
  cannot produce an opcov report at all. The timed runs do not write reports
  and are unaffected.
- **Per-test cost.** tiny-tests is where opcov is slower than PCOV: 85 k
  extra instructions per test, against PCOV's 41 k and turbocov's 8.7 k
  (Xdebug lines 758 k, Xdebug branch/path 2.04 M).

### Profiles

Profiled with `perf record -e cycles:u -g --user-callchains` on a build of the
same commit with line tables and without stripping (the benchmarked build is
stripped), one run each, run directly with the harness's settings.

**tiny-tests (opcov slower than PCOV).** User-space cycles for the whole run:
no coverage 0.15 G, PCOV 0.48 G, opcov 0.90 G. Where opcov's cycles go:

| share of opcov's cycles | where |
|---|---|
| 26.3% | `opcov\stop()`, almost all of it (24.3%) building the returned PHP array: per file the `lines` array, per function an `outcomes` array keyed `o<n>` and a `loops` array. The `o<n>` keys alone (`opcov_add_named_trues`, which formats each key with PHP's `snprintf`, 2.3% in `format_converter`, and inserts it as a new string key) take 7.2%. |
| about 16% | the probe callback chain: `Engine::trigger` 5.6%, `Engine::hit` 4.8%, `probes::probe` 2.8%, `Engine::taken` 2.1%, the VM's `ZEND_USER_OPCODE` handler 1.0% |
| 4.9% | `conflicts::Environment::current()`, which `start()` and `stop()` call on every test to check for conflicting setups: it reads the SAPI name and about a dozen OPcache, Xdebug and PCOV INI entries, parses them and allocates their strings each time |
| 2.3% | `Engine::rearm` in `reset()` and `start()` |

The array opcov's `stop()` returns per test is about twice PCOV's
(17.3 MiB against 8.4 MiB serialized over the run), since it carries
outcomes and loops besides lines, and building it is the largest single
cost. The per-test conflict check is pure overhead: the settings it reads do
not change during a request.

**Carbon (1.51x against turbocov's 1.02x).** User-space cycles: opcov 245 G,
turbocov 162 G. The difference is the probe callback chain: `Engine::hit`
7.9%, `Engine::trigger` 5.7%, `probes::probe` 2.7%, `Engine::taken` 2.0%,
`Engine::decide` 0.9%, `ZEND_USER_OPCODE` 1.1% and `opcov_branch_truth` 0.6%,
about 21% of opcov's cycles (about 51 G), against 0.8% for all of
`turbocov.so`. `stop()` takes 0.6% and the compile hook (including PHP's own
compilation under it) under 3%. A line probe fires once per test in both
extensions; what turbocov does not have is branch sources, which stay armed
and fire on every execution until all their outcomes were seen since the last
`reset()` ([how-it-works.md](how-it-works.md#branch-outcomes)). The branchy
workload shows the mechanism in isolation: after 3,000 calls
`\opcov\get_debug_info()` lists exactly four armed sources in `classify()`
(the two `??` at lines 6 and 7 and the `FE_RESET`/`FE_FETCH` of the
`foreach`) and nothing else armed that runs; they fire about six times per call,
which over 400k calls accounts for the 0.22 s opcov adds there (about 90 ns per
firing, in line with ADR 0002's 37 to 83 ns per armed source run). That
Carbon's extra time is the same mechanism, sources that one test does not
saturate re-firing on every execution, is an inference from the profile; the
harness does not count probe firings.

## Regression check

`tools/measure/bench --driver none --driver opcov --check` fails (exit 1) when,
on any workload, opcov's instructions divided by the no-coverage instructions
of the same run, or the same ratio of cycles, exceed the ratio stored in
`bench/baseline.json` for that PHP version by more than 10%. Ratios, not
absolute counts, are compared, so the baseline holds across machines as long
as the relative cost of opcov's probes does not change; the instruction ratio
also holds under load and at any clock speed. The cycle ratio does not: right
after recording the baseline, `--check --workload tiny-tests --workload
branchy` on the same build passed on instructions and failed on cycles
(tiny-tests 5.52x against a baseline of 4.47x) because the desktop was busy.
Run the check on a quiet machine, or read a cycles-only failure as noise until
it repeats. Record a new baseline with `--update-baseline` after
an intended change and commit `bench/baseline.json`.

## CI

The regression check needs a quiet machine: on a shared CI runner the noise
between runs is larger than 10%. Run it nightly or on a dedicated runner:

```sh
OPCOV_MEASURE_PHP=php85 OPCOV_EXTENSION="$(nix build --print-out-paths .#opcov-php85)/lib/php/extensions/opcov.so" \
    tools/measure/bench --driver none --driver opcov --check
```

The full matrix (all drivers, `--write-docs`) took about 3 h with 5 rounds on
the machine above: Xdebug lines on Carbon (about 10 min per run) and the four
15-minute timeouts (Xdebug branch/path on three workloads, Xdebug lines on
Carbon with `USE_ZEND_ALLOC=0`) make up most of it. It is meant for
refreshing this page, not for every push.
