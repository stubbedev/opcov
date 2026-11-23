# Scale

opcov is built for repositories with millions of lines of PHP and suites with
tens of thousands of tests. This page records how each stage behaves at that
size, with measured numbers.

## Engine

### Corpus and method

Shallow clones (23 November 2026) of symfony/symfony, laravel/framework,
drupal/core, doctrine/orm, doctrine/dbal and sebastianbergmann/phpunit under
`target/measure-cache/scale/` (not committed): **34,607 PHP files, 4,965,369
lines**, after leaving out the 21 files that do not compile together in one
process (parse-error fixtures, scripts that redeclare a function).

Every file is compiled, none is executed: a copy of the corpus has ` return;`
inserted after the opening tag and any `declare`/`namespace` header (on the
same line, so line numbers are unchanged), and a driver `include`s every file
of the copy in one process. Each file thus goes through `zend_compile_file`
and opcov's compile hook exactly as in a test run, its classes and functions
stay in the tables (and their units in opcov) until the end, and only the
main script's statements after `return` become unreachable. The three modes
are PHP without opcov, opcov loaded with `collect_paths` naming an empty
directory (every file unselected), and `collect_paths` naming the corpus
(every file selected). PHP 8.5.13 NTS without OPcache, release build, an
Intel i5-10400F (no SHA extensions) shared with other work; times are the
minimum of five interleaved runs.

### Results

| | without opcov | unselected, before | unselected, after | selected, before | selected, after |
|---|---|---|---|---|---|
| compile, wall | 3.46 s | 4.32 s | 4.06 s | 7.17 s | **6.55 s** |
| compile, user CPU | 2.98 s | 3.33 s | 3.34 s | 5.92 s | 5.62 s |
| compile, system CPU | 0.44 s | 0.97 s | 0.70 s | 1.23 s | 0.90 s |
| peak RSS | 619 MiB | 628 MiB | 631 MiB | 1,256 MiB | **1,049 MiB** |

Selecting every file installs 1,803,495 probes in 198,796 units for 241,290
functions (783,099 basic blocks, 5,911,170 opcodes). opcov's own heap at the
end of the run went from 618 MB to 427 MB: units (probes, sources, loop and
finally tables) 218 → 131 MB, function records with their CFG snapshots and
plans 285 → 196 MB. That is about 75 bytes per probe of unit state and 240
bytes per probe overall.

Cost per file is linear in its size: the added time per byte falls as files
grow (a fixed cost per file dominates small files) and no size class grows
faster than linearly. In the same run, the median file took 2.1 times its
plain compile time selected (2.6 before), the 99th percentile 4.0 (4.9).

What was changed, each measured on the corpus before and after:

- **Plan building** no longer scans every edge per edge or every outcome per
  loop: edges and outcomes are found through their sorted order and loop
  entries through one index of incoming edges. A function with 20,000 loops
  plans in 0.47 s instead of 57 s in a debug build
  (`cfg_plan::planning_many_loops_in_one_function_stays_linear`); match and
  switch arms are ranked by binary search.
- **Probes** shrank from 64 to 32 bytes; loop-entry and finally roles moved to
  per-unit side tables, and the per-opcode probe index (4 bytes per opcode)
  was replaced by a binary search over the probes, which are sorted by opcode.
- **Function keys and paths** share their strings (`FilePath`, `Identifier`
  and `QualifiedName` are reference-counted); a compile parses the file path
  once and each class name once instead of once per function, and builds
  method keys directly instead of formatting and re-parsing them.
- **Placement and layout** use vectors indexed by opcode instead of B-tree
  maps; executable lines are collected into a sorted vector.
- **Selection** resolves a file name with a per-process cache of canonical
  directories and one `lstat` (system CPU of an unselected compile down from
  0.97 s to 0.70 s); the verdict is looked up once per compilation instead
  of once per function, and closures and anonymous classes are ranked only
  when a compilation has a selected function.

### Hashing, path resolution and handler resets

A second round, measured the same way (minimum of five interleaved runs,
same corpus and PHP; the selected runs leave out the one file opcov cannot
key, `symfony/.../Cache/Traits/ValueWrapper.php`, whose class name is not
UTF-8), with `write_report()` timed after the compile:

| | without opcov | unselected, before | unselected, after | selected, before | selected, after |
|---|---|---|---|---|---|
| compile, wall | 3.29 s | 3.85 s | 3.74 s | 6.24 s | **5.32 s** |
| compile, system CPU | 0.43 s | 0.69 s | 0.56 s | | |
| `write_report()` with every file, wall | | | | 0.37 s | 0.52 s |
| peak RSS | 619 MiB | 632 MiB | 631 MiB | 1,061 MiB | 1,095 MiB |

- **SHA-256 left the compile path.** It was about 1.2 s of the selected
  compile (13 % of all samples, `sha2::sha256::compress256`) on this CPU
  without SHA extensions. The compile hook now keeps a 64-bit foldhash
  fingerprint of the compiled bytes, and `write_report()` reads the files
  again on one worker thread per core, checks the fingerprint and hashes
  them: 0.15 s more wall time at report time for 1 s less at compile time.
  [how-it-works.md](how-it-works.md#compiling-a-file) has the trade-off.
- **Absolute file names are not resolved twice.** For an `include` of an
  absolute path the compile hook used to call `zend_resolve_path` (a
  `realpath` with three failing `readlink` and three `stat` calls per file
  on a cache miss) before opcov's own resolution; it now resolves the name
  itself (one `statx` per file and one canonicalization per directory). This
  removed 103,000 `readlink` and 128,000 `newfstatat` calls on the corpus.
- **Handler resets touch fewer opcodes of the handler table.** After a
  compile opcov unregisters its user handler only for the opcodes the new op
  arrays contain (about 50 instead of 255) and computes the plain handlers
  of smart-branch producers with only their own opcodes unregistered (that
  was 510 handler-table writes per unit). Not resetting unselected op arrays
  at all would need opcov's handlers unregistered while PHP compiles, which
  is unsafe ([how-it-works.md](how-it-works.md#compiling-a-file)); the reset
  itself (`zend_vm_set_opcode_handler` per opcode) remains, about 2 % of an
  unselected compile.
- **Hash maps** keyed by pointers, paths and loop activations use foldhash
  with a fixed seed instead of SipHash.

Peak RSS grew by 34 MiB, mostly the per-function frameless twin tables of
the plan and the report's file reads; the probe itself shrank from 32 to 24
bytes plus a 4-byte opcode index.

### Per-test cost

A generated suite of 1,000 files with 100 functions each (1.3 million lines,
2,100,000 probes installed) and 20,000 tests, each calling 50 random functions
and ending with `stop($test)` and `reset()`: every test fires and re-arms about
900 probes, 18,165,446 in total.

| | without opcov | first round, before | first round, after | second round, before | second round, after |
|---|---|---|---|---|---|
| test loop, CPU | 0.96 s | 6.82 s | 6.85 s | 6.18 s | **4.95 s** |
| `stop()` + `reset()`, median per test | | 192 µs | 175 µs | 158 µs | **113 µs** |
| `stop()` + `reset()`, 99th percentile | | 327 µs | 307 µs | 256 µs | 196 µs |
| peak RSS | 184 MiB | 948 MiB | 798 MiB | 798 MiB | 804 MiB |

(The second round ran on a less loaded machine, 0.84 s without opcov, so its
"before" is lower than the first round's "after" for the same code.) The added CPU per fired
and re-armed probe fell from about 294 ns to 226 ns. The probe's own cost is
dominated by cache misses on the unit, the opcode index and the probe in a
2.1-million-probe working set; the second round made the lookup touch a
4-byte opcode array instead of 32-byte probes, recorded each covered item
when the probe fires (the unit and probe are in cache then) instead of
visiting them again in `stop()`, sorts covered items as packed 128-bit keys,
groups them without B-tree maps, caches each function's key text, and builds
the PHP arrays of lines and outcomes in one engine call each.

A micro-benchmark that runs one function of 200,000 lines once per test
(every line a probe, fired and re-armed each test):

| per line | without opcov | before | after |
|---|---|---|---|
| running the function | 12.8 ns | 85.0 ns | **38.8 ns** |
| `stop()` + `reset()` | | 71.6 ns | **48.0 ns** |

Looking up the firing opcode right after the unit's previous probe, before
falling back to the binary search, makes forward execution of a large
function cost one comparison per probe.

`stop()` and `reset()` depend only on the probes that fired; the 2.1 million
installed probes do not enter either. `stop()` groups the hits by file and
function id and sorts by path and key once per file instead of comparing
paths per hit, and a test's first attribution is stored without re-sorting.
Attribution keeps one sorted list of 12-byte items per test (16 before).
This benchmark re-fires every probe in every test, the worst case; in a real
suite most probes fire once per test that reaches them.

## Reports and CLI

### What bounds memory

- **`opcov\write_report()`** writes the report file by file through a buffered
  writer, straight from the extension's per-test attribution. It never builds
  the whole report in memory. To turn per-test hit lists into per-file hit
  lists it counts the hits of every file, sorts the files by path and gathers
  the hits of a batch of consecutive files in one pass over the attribution.
  A batch holds at most 2^24 hits (256 MiB), or one file when a single file
  has more, so the memory beyond the attribution itself is bounded by that
  budget plus the largest file entry, and the number of passes grows with the
  attribution, not with the number of files. The bytes are those of the
  in-memory writer (the same `ReportWriter` in `opcov-core` writes both, and
  `merge` uses it too).
- **Reading.** Every reader in `opcov-core` (`Report::read`, the CLI,
  `merge`) is the same pull reader: a byte scanner splits the `files` object
  into one raw entry per file, which is decoded and validated on its own. The
  header is read once; `merge` and the CLI read only the header up front.
- **CLI.** `convert`, `gate` and `doctor` decode and process entries on one
  worker per core while the reader runs ahead, with at most 64 entries per
  worker in flight, and emit results in report order, so the output is the
  same for any core count. Source files are read, hash-checked and parsed
  with mago on the workers. Clover, Cobertura and HTML read the report twice
  (totals first); Clover spools its `<file>` elements to an unlinked temporary
  file to group them by package. `gate` decodes only the changed files'
  entries. `merge` reads and decodes each input on its own thread.

Peak memory of the CLI is therefore set by the worker count and the size of
the largest entries, not by the number of files; `write_report` adds at most
its batch budget to the attribution the extension already holds.

### Fixtures

Generated under `target/measure-cache/report-scale/` (not committed):

- **A**: a report over a real multi-million-line corpus (Laravel, Symfony,
  Drupal core, Doctrine DBAL/ORM, PHPUnit: 34,628 PHP files, 4.97 million
  source lines; five files that do not parse are left out), generated by
  `cargo run --release -p opcov-cli --example report_scale -- <corpus>
  <report.json> 50000 8 60`: 34,623 files, 2,830,148 executable lines,
  311,824 branches, 50,000 tests, each file hit by the ~23 tests whose window
  covers it (60 % of its lines per test). 337 MB.
- **B**: the same with a window of 32 files (~4x the attribution): 1,063 MB.
- **E**: reports written by the extension itself, from a generated corpus of
  20,000 and 32,000 files (10 functions each, `foreach`, `if`/`else`, `??`)
  driven by a script that runs 50,000 named tests, each calling the 90
  functions of 9 neighbouring files: 1.6 and 2.56 million executable lines,
  467 and 556 MB.

### Results

Wall time and peak RSS on a 6-core (12-thread) i5-10400F shared with other
builds (load average around 8), with `--release`, before (the code at
`7dcb4df`) and after. Every output is byte-identical between the two.

| Command | Fixture | Before | After |
|---|---|---|---|
| `convert --format lcov` | A | 4.83 s, 457 MB | 1.17 s, 76 MB |
| `convert --format clover` | A | 5.34 s, 630 MB | 2.88 s, 135 MB |
| `convert --format cobertura` | A | 9.85 s, 627 MB | 2.72 s, 151 MB |
| `convert --format html` (reads 34k sources) | A | 11.06 s, 813 MB | 2.99 s, 156 MB |
| `doctor --report` (parses 34k sources) | A | 8.08 s, 526 MB | 3.35 s, 321 MB |
| `merge` (A with itself) | A | 13.40 s, 38 MB | 7.20 s, 68 MB |
| `convert --format lcov` | B | 9.83 s, 926 MB | 2.48 s, 140 MB |
| `convert --format clover` | B | 11.18 s, 1,099 MB | 5.23 s, 205 MB |
| `convert --format cobertura` | B | 14.24 s, 1,096 MB | 6.27 s, 195 MB |
| `convert --format html` | B | 15.66 s, 1,282 MB | 6.18 s, 236 MB |
| `doctor --report` | B | 17.11 s, 996 MB | 5.51 s, 358 MB |
| `gate`, 203 changed files | B | 8.79 s, 945 MB | 1.53 s, 65 MB |
| `merge` (B with itself) | B | 29.10 s, 83 MB | 11.70 s, 151 MB |

| `opcov\write_report()` | Before | After |
|---|---|---|
| E, 1.6 M lines, 467 MB | 6.34 s, +1,556 MB over the attribution | 5.04 s, +262 MB |
| E, 2.56 M lines, 556 MB | 8.07 s, +1,481 MB (peak 4.27 GB) | 7.45 s, +265 MB (peak 2.15 GB) |

"+MB" is the growth of the process's peak resident set during the call
(`VmHWM` after minus `VmRSS` before). The peak before the call differs between
the two builds because the engine's own attribution storage changed in the
same period; the growth is the part `write_report` owns.

From A to B the attribution grows 3.15x and the files stay the same: time grows
roughly linearly (the scan runs at about 700 MB/s, decoding and rendering on
the workers), and memory grows with the size of the in-flight entries only.

### Not yet parallel

- `opcov\write_report()` builds and serializes entries on the PHP thread
  (about 3 µs per executable line here).
- `merge` unions and serializes on one thread; its inputs are read and
  decoded in parallel.
- The per-file views (`convert`, `gate`) find a line's function by scanning
  the file's functions, which is quadratic within one file; it does not grow
  with the number of files.
