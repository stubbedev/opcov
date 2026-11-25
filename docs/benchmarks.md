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
| `carbon` | a large unit suite: Carbon 3.15.1 (the results below) or the corpus pin `e4f5856`, which changes only tests and docblocks, 6,349 tests, `src/Carbon` measured |
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
PHP 8.5.13, median of 3 runs. Machine: cpu Intel(R) Core(TM) i5-10400F CPU @ 2.90GHz, logical cpus 12, governor powersave, turbo on (intel_pstate), kernel Linux 6.18.64, memory 31.3 GiB, perf_event_paranoid 2, load average at start 21.45 17.37 10.50.

Drivers: PHP 8.5.13, opcov 0.1.0, pcov 1.0.12, xdebug 3.5.3, xdebug 3.5.3, turbocov 0.1.1.

Counters: `instructions:u`, `cycles:u`, `branches:u`, `branch-misses:u`, `cache-misses:u`, `page-faults:u`, `task-clock:u`, as `perf stat` reported them.

#### branchy

Synthetic branch-heavy CPU loop: one collection around 400k calls of a function with if/elseif, &&/||, ??, ?:, match, switch, foreach and ?->; 2 tests.

| driver | instructions | x none | cycles | x none | branch misses | cache misses | page faults | task-clock s | wall s (min-max) | x none | peak RSS MiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| no coverage | 2.374 G | 1.00 | 1.085 G | 1.00 | 2.510 M | 602.558 k | 3.381 k | 0.283 | 0.305 (0.284-0.308) | 1.00 | 37.3 |
| opcov (lines, outcomes, loops) | 4.379 G | 1.84 | 1.830 G | 1.69 | 3.787 M | 561.374 k | 3.433 k | 0.460 | 0.475 (0.473-0.491) | 1.56 | 38.6 |
| PCOV | 8.546 G | 3.60 | 3.165 G | 2.92 | 8.543 M | 596.074 k | 3.394 k | 0.788 | 0.804 (0.802-0.814) | 2.63 | 37.4 |
| Xdebug (lines) | 40.946 G | 17.25 | 17.239 G | 15.88 | 47.165 M | 1.326 M | 3.527 k | 4.269 | 4.285 (4.226-4.305) | 14.03 | 38.1 |
| Xdebug (branch) | > 15 min |  |  |  |  |  |  |  |  |  |  |
| turbocov | 2.402 G | 1.01 | 1.074 G | 0.99 | 2.736 M | 527.149 k | 3.393 k | 0.278 | 0.293 (0.285-0.304) | 0.96 | 37.4 |

| driver | allocation calls (Zend MM) | x none | bytes allocated (Zend MM) | allocation calls (USE_ZEND_ALLOC=0) | x none | bytes allocated (USE_ZEND_ALLOC=0) | PHP peak MiB | payload MiB | report MiB |
|---|---|---|---|---|---|---|---|---|---|
| no coverage | 40.380 k | 1.00 | 5.9 MiB | 4.578 M | 1.00 | 565.8 MiB | 2.0 | 0.00 | n/a |
| opcov (lines, outcomes, loops) | 41.248 k | 1.02 | 6.0 MiB | 4.580 M | 1.00 | 566.2 MiB | 2.0 | 0.00 | 0.00 |
| PCOV | 40.455 k | 1.00 | 5.9 MiB | 4.578 M | 1.00 | 567.0 MiB | 2.0 | 0.00 | n/a |
| Xdebug (lines) | 848.339 k | 21.01 | 18.9 MiB | 46.358 M | 10.13 | 5853.9 MiB | 2.0 | 0.00 | n/a |
| Xdebug (branch) | > 15 min |  |  |  |  |  |  |  |  |
| turbocov | 40.472 k | 1.00 | 5.9 MiB | 4.578 M | 1.00 | 566.0 MiB | 2.0 | 0.00 | n/a |

#### tiny-tests

20,000 tiny tests touching the same class, each bracketed by start/stop and the driver's per-test reset (opcov `reset()`, PCOV `clear()`); 20001 tests.

| driver | instructions | x none | cycles | x none | branch misses | cache misses | page faults | task-clock s | wall s (min-max) | x none | peak RSS MiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| no coverage | 303.188 M | 1.00 | 155.040 M | 1.00 | 628.107 k | 455.907 k | 3.458 k | 0.048 | 0.063 (0.062-0.079) | 1.00 | 37.6 |
| opcov (lines, outcomes, loops) | 1.116 G | 3.68 | 488.368 M | 3.15 | 1.292 M | 707.027 k | 6.489 k | 0.136 | 0.155 (0.151-0.157) | 2.48 | 49.8 |
| PCOV | 1.123 G | 3.70 | 492.312 M | 3.18 | 1.616 M | 548.583 k | 3.471 k | 0.132 | 0.147 (0.145-0.148) | 2.35 | 37.8 |
| Xdebug (lines) | 15.472 G | 51.03 | 8.245 G | 53.18 | 20.506 M | 662.135 k | 3.897 k | 2.025 | 2.040 (2.032-2.054) | 32.56 | 39.8 |
| Xdebug (branch) | 41.081 G | 135.50 | 59.368 G | 382.92 | 211.325 M | 767.703 M | 24.506 k | 14.586 | 14.616 (14.603-15.108) | 233.27 | 119.7 |
| turbocov | 477.161 M | 1.57 | 219.686 M | 1.42 | 704.955 k | 462.103 k | 3.476 k | 0.065 | 0.080 (0.078-0.081) | 1.28 | 37.7 |

| driver | allocation calls (Zend MM) | x none | bytes allocated (Zend MM) | allocation calls (USE_ZEND_ALLOC=0) | x none | bytes allocated (USE_ZEND_ALLOC=0) | PHP peak MiB | payload MiB | report MiB |
|---|---|---|---|---|---|---|---|---|---|
| no coverage | 40.380 k | 1.00 | 5.9 MiB | 264.836 k | 1.00 | 30.3 MiB | 2.0 | 0.04 | n/a |
| opcov (lines, outcomes, loops) | 81.243 k | 2.01 | 41.2 MiB | 946.534 k | 3.57 | 199.3 MiB | 2.0 | 17.26 | 4.29 |
| PCOV | 40.455 k | 1.00 | 5.9 MiB | 464.997 k | 1.76 | 1457.2 MiB | 4.0 | 8.39 | n/a |
| Xdebug (lines) | 31.347 M | 776.30 | 1119.0 MiB | 34.572 M | 130.54 | 1602.7 MiB | 2.0 | 18.30 | n/a |
| Xdebug (branch) | 62.499 M | 1547.76 | 5584.7 MiB | 86.514 M | 326.67 | 7592.2 MiB | 4.0 | 208.36 | n/a |
| turbocov | 40.486 k | 1.00 | 5.9 MiB | 385.078 k | 1.45 | 81.7 MiB | 2.0 | 5.72 | n/a |

#### carbon

Carbon's PHPUnit suite (large unit suite, many small tests, `src/Carbon` measured); 6349 tests.

| driver | instructions | x none | cycles | x none | branch misses | cache misses | page faults | task-clock s | wall s (min-max) | x none | peak RSS MiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| no coverage | 225.036 G | 1.00 | 160.364 G | 1.00 | 1.228 G | 169.811 M | 140.703 k | 40.623 | 41.691 (40.390-48.741) | 1.00 | 190.6 |
| opcov (lines, outcomes, loops) | 333.479 G | 1.48 | 227.012 G | 1.42 | 1.717 G | 112.452 M | 160.997 k | 55.787 | 56.872 (56.578-57.095) | 1.36 | 270.9 |
| PCOV | 535.605 G | 2.38 | 349.671 G | 2.18 | 3.474 G | 360.898 M | 142.854 k | 85.617 | 86.702 (86.144-90.429) | 2.08 | 197.5 |
| Xdebug (lines) | 4971.229 G | 22.09 | 2389.669 G | 14.90 | 9.724 G | 621.804 M | 145.522 k | 581.137 | 582.294 (570.555-739.359) | 13.97 | 207.8 |
| Xdebug (branch) | > 15 min |  |  |  |  |  |  |  |  |  |  |
| turbocov | 230.560 G | 1.02 | 161.144 G | 1.00 | 1.247 G | 77.463 M | 141.839 k | 39.784 | 40.873 (40.449-41.732) | 0.98 | 195.6 |

| driver | allocation calls (Zend MM) | x none | bytes allocated (Zend MM) | allocation calls (USE_ZEND_ALLOC=0) | x none | bytes allocated (USE_ZEND_ALLOC=0) | PHP peak MiB | payload MiB | report MiB |
|---|---|---|---|---|---|---|---|---|---|
| no coverage | 355.888 k | 1.00 | 160.3 MiB | 175.994 M | 1.00 | 29563.2 MiB | 148.5 | 0.01 | n/a |
| opcov (lines, outcomes, loops) | 3.154 M | 8.86 | 722.8 MiB | 181.411 M | 1.03 | 33222.4 MiB | 148.5 | 130.95 | 28.90 |
| PCOV | 355.962 k | 1.00 | 160.3 MiB | 176.722 M | 1.00 | 32609.0 MiB | 154.5 | 243.22 | n/a |
| Xdebug (lines) | 455.225 M | 1279.12 | 21824.9 MiB | 2.880 G | 16.36 | 526547.0 MiB | 150.5 | 220.20 | n/a |
| Xdebug (branch) | > 15 min |  |  |  |  |  |  |  |  |
| turbocov | 473.169 k | 1.33 | 204.2 MiB | 176.727 M | 1.00 | 32405.3 MiB | 148.5 | 60.74 | n/a |

#### laravel-query-builder

Spatie/laravel-query-builder's Pest suite: Laravel feature tests through Orchestra Testbench on in-memory SQLite (`src` measured); 367 tests.

| driver | instructions | x none | cycles | x none | branch misses | cache misses | page faults | task-clock s | wall s (min-max) | x none | peak RSS MiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| no coverage | 20.826 G | 1.00 | 17.404 G | 1.00 | 153.997 M | 104.588 M | 19.791 k | 4.486 | 4.504 (4.435-4.670) | 1.00 | 107.2 |
| opcov (lines, outcomes, loops) | 21.121 G | 1.01 | 18.416 G | 1.06 | 157.031 M | 143.763 M | 20.433 k | 4.794 | 4.811 (4.625-5.351) | 1.07 | 110.5 |
| PCOV | 58.158 G | 2.79 | 36.823 G | 2.12 | 290.857 M | 194.443 M | 20.064 k | 9.358 | 9.382 (9.034-9.603) | 2.08 | 107.1 |
| Xdebug (lines) | 172.911 G | 8.30 | 96.949 G | 5.57 | 500.981 M | 286.285 M | 21.878 k | 24.218 | 24.237 (24.148-24.366) | 5.38 | 114.7 |
| Xdebug (branch) | > 15 min |  |  |  |  |  |  |  |  |  |  |
| turbocov | 21.100 G | 1.01 | 17.447 G | 1.00 | 154.847 M | 108.410 M | 19.850 k | 4.508 | 4.526 (4.505-4.670) | 1.01 | 107.5 |

| driver | allocation calls (Zend MM) | x none | bytes allocated (Zend MM) | allocation calls (USE_ZEND_ALLOC=0) | x none | bytes allocated (USE_ZEND_ALLOC=0) | PHP peak MiB | payload MiB | report MiB |
|---|---|---|---|---|---|---|---|---|---|
| no coverage | 583.039 k | 1.00 | 220.9 MiB | 18.444 M | 1.00 | 3213.2 MiB | 62.5 | 0.00 | n/a |
| opcov (lines, outcomes, loops) | 611.058 k | 1.05 | 227.6 MiB | 18.507 M | 1.00 | 3243.4 MiB | 62.5 | 1.43 | 0.24 |
| PCOV | 583.113 k | 1.00 | 220.9 MiB | 18.456 M | 1.00 | 3289.6 MiB | 62.5 | 1.05 | n/a |
| Xdebug (lines) | 52.695 M | 90.38 | 2333.0 MiB | 96.278 M | 5.22 | 19374.7 MiB | 66.5 | 15.90 | n/a |
| Xdebug (branch) | > 15 min |  |  |  |  |  |  |  |  |
| turbocov | 584.008 k | 1.00 | 221.4 MiB | 18.485 M | 1.00 | 3236.9 MiB | 62.5 | 0.62 | n/a |
<!-- bench:php85:end -->

### Reading the results

Instructions with coverage divided by instructions without, median of 3
runs (cycles in parentheses):

| workload | opcov | PCOV (lines) | turbocov (lines) | Xdebug lines | Xdebug branch/path |
|---|---|---|---|---|---|
| laravel-query-builder | **1.01** (1.06) | 2.79 (2.12) | 1.01 (1.00) | 8.30 (5.57) | over 15 min, timed out |
| carbon | **1.48** (1.42) | 2.38 (2.18) | 1.02 (1.00) | 22.09 (14.90) | over 15 min, timed out |
| branchy | **1.84** (1.69) | 3.60 (2.92) | 1.01 (0.99) | 17.25 (15.88) | over 15 min, timed out |
| tiny-tests | **3.68** (3.15) | 3.70 (3.18) | 1.57 (1.42) | 51.03 (53.18) | 135.50 (382.92) |

opcov collects lines, branch outcomes and loops with per-test attribution;
PCOV and turbocov collect lines only.

- **Framework workload.** On the Laravel feature tests opcov's full
  collection executes 1% more instructions than no coverage, which meets the
  epic's target of branch coverage within 1.2x of no coverage. PCOV executes
  2.79 times the instructions for lines only.
- **Large unit suite.** On Carbon opcov executes 108 G instructions more than
  no coverage over 6,349 tests (1.48x), PCOV 311 G (2.38x), turbocov 6 G
  (1.02x) for lines only. Most of opcov's extra work is in the probe callback
  chain of branch sources that a test never saturates, so they fire on every
  call.
- **Per-test cost.** On 20,000 tiny tests opcov executes 3.68 times the
  instructions of no coverage, about 41 k extra per test, level with PCOV
  (3.70x) while also returning outcomes and loops.
- **Xdebug branch/path.** The `xdebug-branch` driver passes
  `XDEBUG_CC_BRANCH_CHECK`, which makes Xdebug record branches and enumerate
  paths. Its first run exceeded the 15-minute limit on branchy, Carbon and
  the Laravel suite, so those rows show the timeout and it was not run again
  for that workload. Only tiny-tests finished, at 135.5x no coverage. On a
  real branch with PHPUnit, Xdebug path coverage took 314 times as long as
  opcov ([gate.md](gate.md#results)).
- **Allocations.** With Zend MM (the normal setup) opcov makes 1.05x the
  `malloc`-family calls of no coverage on Laravel, 1.02x on branchy, 2.01x on
  tiny-tests and 8.86x on Carbon (3.15 M calls against 0.36 M); no allocation
  happens per probe fire or per watch resolution, so what remains is mostly
  compiling and instrumenting files. With `USE_ZEND_ALLOC=0`, where PHP's own
  per-object allocations go through `malloc` too, opcov adds 1.00x to 1.03x
  on the real workloads.
- **Memory.** opcov's peak RSS is a few percent to about 40% above no
  coverage (110 against 107 MiB on Laravel, 271 against 191 MiB on Carbon),
  while PHP's own peak (`memory_get_peak_usage(true)`) is unchanged: opcov's
  state lives outside Zend MM.
- **Reports.** Every workload writes a report; Carbon's is 28.9 MiB, with one
  entry per distinct `eval()`'d source.

### History

The first counter run (6d4f418) measured tiny-tests at 6.61x, above PCOV.
Its profile put 26% of opcov's cycles in `stop()` building the returned
array (with `snprintf`-formatted keys), 4.9% in re-reading INI settings on
every `start()` and `stop()`, and one `Vec` allocation per resolved successor
watch (26.2 M calls on Carbon). Fixing those (#40) brought tiny-tests to
3.68x, branchy from 1.99x to 1.84x and Carbon from 1.56x to 1.48x.

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
