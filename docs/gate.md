# Gating a change on coverage

`opcov gate` checks the functions a change touches against the coverage of a
test run: lines no test ran, branch outcomes no test took, and loops that no
test ran zero times or two or more times ([cli.md](cli.md#opcov-gate)). This
page shows how to run it in CI, compares it with a traditional gate built on
Xdebug's branch and path coverage on a real branch, and says which rules of
such a gate become plain coverage checks with opcov and which stay
mutation-based.

## Running the gate in CI

The gate needs two inputs: the canonical report of the test run and the
changed lines.

1. Install the extension ([install.md](install.md)) and the PHPUnit package
   (`composer require --dev opcov/phpunit`), and register the extension in
   the PHPUnit configuration with the report as `output`
   ([phpunit.md](phpunit.md#register-the-extension)):

   ```xml
   <extensions>
       <bootstrap class="OpCov\PHPUnit\Extension">
           <parameter name="output" value="build/opcov.json"/>
       </bootstrap>
   </extensions>
   ```

2. Run the tests through the launcher, which turns `<source>` into the opcov
   INI settings ([phpunit.md](phpunit.md#with-the-launcher)):

   ```sh
   vendor/bin/opcov-phpunit vendor/bin/phpunit
   ```

3. Diff the branch against the commit it forked from and gate on it. Use the
   merge base (`origin/main...HEAD`, three dots), so commits that landed on
   `main` since the fork are not counted as changes of the branch; fetch enough
   history for the merge base to exist (`fetch-depth: 0` in
   `actions/checkout`). Run the gate from the directory the report's paths are
   relative to (the working directory of the test run, unless the extension's
   `root` parameter says otherwise), or pass that directory with `--base`:

   ```sh
   git fetch origin main
   git diff -U0 origin/main...HEAD | opcov gate --report build/opcov.json --diff -
   ```

   Exit code 0 passes, 1 means findings, 2 means the gate could not run (an
   unreadable report, a source file that changed after the test run). Add
   `--format json` for a machine-readable result.

With paratest, every worker writes its own report and `opcov merge` joins
them before the gate ([phpunit.md](phpunit.md#parallel-runs-with-paratest)):

```xml
<parameter name="output" value="build/opcov.{TEST_TOKEN}.json"/>
```

```sh
vendor/bin/opcov-phpunit vendor/bin/paratest -c phpunit-paratest.xml -p 8
opcov merge build/opcov.*.json -o build/opcov.json
git diff -U0 origin/main...HEAD | opcov gate --report build/opcov.json --diff -
```

Pest 4 reads the same PHPUnit configuration: run
`vendor/bin/opcov-phpunit vendor/bin/pest` (or `--parallel`, with the
`{TEST_TOKEN}` output and `opcov merge`).

A GitHub Actions job:

```yaml
gate:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
      with:
        fetch-depth: 0
    - run: composer install --no-interaction
    - run: vendor/bin/opcov-phpunit vendor/bin/phpunit
    - run: git diff -U0 "origin/${{ github.base_ref }}...HEAD" | opcov gate --report build/opcov.json --diff -
```

### Files no test loads

The extension measures the files PHP compiles during the run, so a changed
file that no test loads is not in the report. The gate fails on it (exit code
1) when the run would have measured it, that is when it is a `.php` file below
the report's `collect_paths` and below none of its `exclude_paths`: it is
reported as "never loaded by any test" with its changed lines, and counts as
one finding. php-code-coverage does the same by default
(`includeUncoveredFiles`), so an Xdebug-based gate fails on the same files.
The comparison below does not hit this, because the full suite loads every
changed file; running only `--filter ByteStringTest` leaves
`Slugger/AsciiSlugger.php` unloaded, which the Xdebug gate reports with 54
uncovered lines.

Changed files the run cannot measure (not `.php`, outside the collect paths or
excluded by an exclude path, outside the report's root, deleted) are listed
under "not in the report" and are not findings. A report with an
`exclude_pattern` is the one case the gate cannot decide, since the pattern is
a PCRE only PHP evaluates; such files are listed with that reason
([cli.md](cli.md#changed-files-the-report-does-not-contain)).

### Callers of changed members

The gate checks the functions the diff touches. A change to a method can break
its callers, which the diff does not touch; opcov builds no call graph, so pass
the callers you want checked as extra `--files path:start-end` entries.

### Running only the tests the change can reach

The gate looks only at the changed functions, so only the tests that reach
them need to run. `opcov impact` picks them from a baseline report, a run of
the commit the branch starts from that the main branch's CI keeps anyway
([cli.md](cli.md#opcov-impact)):

```sh
git diff -U0 "$BASELINE_COMMIT" > branch.diff
status=0
filter="$(opcov impact --report baseline.json --diff branch.diff --ignore docs --format filter)" || status=$?
case "$status" in
  0) [ -z "$filter" ] || vendor/bin/opcov-phpunit vendor/bin/phpunit --filter "$filter" ;;
  1) vendor/bin/opcov-phpunit vendor/bin/phpunit ;;
  *) exit "$status" ;;
esac
if [ -f build/opcov.json ]; then
  opcov gate --report build/opcov.json --diff branch.diff
else
  opcov impact --report baseline.json --diff branch.diff --ignore docs --format json \
    | jq -e '[.changes[].functions // [] | .[]] | length == 0'
fi
```

Exit code 0 is a selection that bounds the change; exit code 1 means a change
the baseline cannot bound (a constant, a new method, a test helper, a non-PHP
file), and the full suite runs. A baseline of another commit is refused (exit
code 2), since it would choose the wrong tests.

An empty filter means no baseline test reached a changed function, so no test
reaches it on the branch either and there is no report to gate. The change
passes only when it changed no function at all (comments, whitespace, ignored
or never-loaded files); otherwise every changed function is uncovered, which
the last command turns into a failure and `impact`'s text output lists.

In GitHub Actions the baseline is the main branch's last report, uploaded as
an artifact with a file `commit` holding the commit it was built from;
download it (for example with `actions/download-artifact` and the run id of
that workflow run) and run the script above with
`BASELINE_COMMIT="$(cat baseline/commit)"`. Check out with `fetch-depth: 0` so
the commit exists.

## Comparison with an Xdebug-based gate

`tools/measure/gate/` runs a real branch of a pinned corpus project through
both gates and diffs their findings:

```sh
tools/measure/gate/gate collect --rounds 3   # run the suite without coverage, under Xdebug and under opcov, 3 times each
tools/measure/gate/gate compare              # diff the findings, print the table below
tools/measure/gate/gate compare --write-docs # and rewrite the results section of this page
```

- **The branch** (`tools/measure/gate/branch.json`): a range of commits of a
  project from `tools/measure/corpus.json`, whose head is the commit the
  corpus pins, so the pinned checkout and its locked dependencies run the
  changed code. The changed lines are `git diff -U0 base head`.
- **The Xdebug gate** (`tools/measure/gate/php/reference.php`): the test suite
  runs under Xdebug 3 with PHPUnit's own coverage, `--coverage-php
  coverage.cov --path-coverage`, as a traditional gate collects it. Xdebug is
  loaded from an INI file in `PHP_INI_SCAN_DIR`, as from `php.ini` in CI, so
  the child processes of process-isolated tests load it too. The
  reference script loads the serialized php-code-coverage object, parses the
  changed files with `nikic/php-parser`, selects the changed functions by the
  same rules as `opcov gate` (the innermost function around a changed line or
  around a run of removed lines that no new lines replace, plus the closures
  inside it) and reports per function
  the lines php-code-coverage counts as executable and not covered, and the
  branches never taken. A branch is Xdebug's notion: a basic block of the
  function, taken when any test executed it. php-code-coverage records hits
  per block, not per edge, so its reports show the blocks a test ran.
- **The opcov gate**: the same suite runs through the `opcov-phpunit` launcher
  with `OpCov\PHPUnit\Extension` added to a copy of the project's PHPUnit
  configuration; then `opcov gate --report opcov.json --diff branch.diff
  --format json`.
- **The diff of the findings**
  (`tools/measure/gate/php/src/Comparison.php`) matches the two gates per
  changed function:
  - lines: uncovered for both, or for one of them (then whether the other
    counts the line as covered or not executable);
  - every Xdebug branch never taken must be matched by an opcov finding: an
    uncovered line inside the block, or an unseen outcome on the line of the
    decision that leads into it, or the block is only reachable from other
    blocks that never ran. A block entered by an outcome opcov reports but
    excludes from verdicts (`Engine`: the `JMP_FRAMELESS` check of an
    unqualified call in a namespace; `LoopEnter`/`LoopSkip` of `foreach`) is
    an Xdebug-only finding explained by rule;
  - every unseen opcov outcome is on an uncovered line, leads into an Xdebug
    block that never ran, is a `??`/`?:`/`?->` side (Xdebug compiles no branch
    for `COALESCE`, `JMP_SET`, `JMP_NULL`), or is an edge whose target block
    ran through another edge (the implicit `else` of an `if`, the
    short-circuit of `&&`/`||` into a join block that the other operand also
    reaches, a loop exit): Xdebug's block-level view counts that block as
    taken;
  - loops never run zero times or never run two or more times are opcov-only
    by rule: Xdebug cannot express them;
  - an unseen outcome of a construct on a covered line whose outcomes were
    all never taken, on a line with a never-taken `Engine` outcome before it,
    is a copy of the construct in the `JMP_FRAMELESS` fallback (PHP compiles
    the arguments of an unqualified frameless call in a namespace twice); it
    is counted separately and needs an entry, because no test can take it.

  Every other difference needs an entry in
  `tools/measure/gate/explanations.json` (category, file, function, lines,
  reason). `compare` exits 1 on an unexplained difference or an entry that
  matches nothing.
- **Matching by line.** Xdebug's branch numbers are opcode indexes of an op
  array compiled with `EXT_STMT` and with jump tables, opcov's sites are
  indexes of an op array compiled without either, so the two are matched by
  line, not by opcode.
- **Timing.** Each coverage step is the whole PHPUnit run that produces the
  gate's input: no coverage (the baseline), Xdebug with path coverage writing
  `coverage.cov`, opcov writing `opcov.json`. The three run one after another
  in each round, under the same clean environment (OPcache off,
  `memory_limit=-1`), and the table reports the median wall-clock time of the
  rounds. The gate evaluations are timed separately.

### Results

<!-- gate:results:begin -->
Branch: the symfony/string 8.1 patch releases v8.1.0 to v8.1.10, `afd5944f4005..6c6110979c01` of [symfony/string](https://github.com/symfony/string) (2932 tests at the head). Versions: PHP 8.5.13, opcov 0.1.0, Xdebug 3.5.3, phpunit/php-code-coverage 12.5.x-dev@eb15ced29842, phpunit/phpunit 12.5.x-dev@0cc607272780; opcov built from commit c41de9bbf440 with uncommitted changes to crates/. Machine at the start of the collection: cpu Intel(R) Core(TM) i5-10400F CPU @ 2.90GHz, logical cpus 12, governor powersave, turbo on (intel_pstate), kernel Linux 6.18.64, memory 31.3 GiB, load average at start 2.32 1.35 1.20.

#### Coverage step, median of 3 runs

| step | median s | min s | max s | x no coverage | peak RSS MiB |
|---|---|---|---|---|---|
| no coverage | 1.0 | 1.0 | 1.1 | 1.00 | 77 |
| Xdebug path coverage, php-code-coverage `--coverage-php --path-coverage` | 370.8 | 368.0 | 382.4 | 362.47 | 781 |
| opcov, `opcov-phpunit` launcher and `OpCov\PHPUnit\Extension` | 1.2 | 1.2 | 1.9 | 1.16 | 84 |

Xdebug path coverage takes 313.8x as long as opcov.

Evaluating the gate from the finished coverage (median): `opcov gate` 0.025 s, the Xdebug reference gate 0.406 s (loading the `.cov` file).

#### Findings

35 changed functions checked by both gates. Xdebug's path coverage enumerated 14839 paths through them, 14699 never taken; opcov has no paths and they are not compared.

| finding | reported by | count | explained |
|---|---|---|---|
| uncovered line, both | both | 16 | agree |
| Xdebug branch never ran, opcov has an uncovered line in it | both | 18 | agree |
| Xdebug branch never ran, opcov has an unseen outcome into it | both | 7 | agree |
| Xdebug branch never ran, entered by an outcome opcov excludes from verdicts (`Engine`, `LoopEnter`/`LoopSkip`) | Xdebug only | 47 | by rule |
| Xdebug branch never ran, no opcov finding | Xdebug only | 2 | per entry |
| unseen outcome on a line no test ran | both | 2 | agree |
| unseen outcome into an Xdebug branch that never ran | both | 18 | agree |
| unseen `??`/`?:`/`?->` outcome (Xdebug has no branch) | opcov only | 5 | by rule |
| unseen outcome whose target block ran through another edge (Xdebug counts blocks) | opcov only | 9 | by rule |
| loop never ran zero times | opcov only | 9 | by rule |
| loop never ran two or more times | opcov only | 2 | by rule |

Differences explained per entry (`tools/measure/gate/explanations.json`):

| where | difference | reason |
|---|---|---|
| Resources/data/wcswidth_table_wide.php:12 {main}@Resources/data/wcswidth_table_wide.php | Xdebug branch never ran, no opcov finding (block at opcode 0, lines 12-1204, entered from line(s) none) | Xdebug 3.5.3 artifact: the file's only block (`return [...]`) ran in `ByteStringTest::testWidth#3`, the test both drivers attribute line 12 to, but Xdebug reports its branch `hit` as 0 because the file is `require`d from inside a method (`self::$tableWide ??= require ...` in `AbstractUnicodeString::wcswidth`). The same file required from top-level code reports `hit` 1. Not a coverage gap. |
| Resources/data/wcswidth_table_zero.php:12 {main}@Resources/data/wcswidth_table_zero.php | Xdebug branch never ran, no opcov finding (block at opcode 0, lines 12-1504, entered from line(s) none) | Same Xdebug artifact as wcswidth_table_wide.php: the only block ran in `ByteStringTest::testWidth#3` (line 12 covered for both drivers), required from `AbstractUnicodeString::wcswidth`, and Xdebug reports the branch as never hit. |

Verdict: every difference is explained. 0 unexplained difference(s), 0 stale explanation(s).
<!-- gate:results:end -->

What the run shows:

- **Speed.** The coverage step under opcov costs about as much as the test
  suite without coverage (1.16x); under Xdebug with path coverage it takes
  314 times as long as under opcov, far beyond the 10x target. No other
  measurement ran during the collection; the load average at its start
  includes the release build that preceded it. Peak memory follows the same
  pattern (84 MiB against 781 MiB).
- **Same set of functions.** Both gates selected the same changed functions
  (no "selected by one gate only" row).
- **opcov is as strict or stricter.** Every Xdebug finding is matched by an
  opcov finding, except the blocks behind `Engine` outcomes (the never-taken
  `JMP_FRAMELESS` fallback of unqualified calls like `str_contains()` in a
  namespace, which no test can take and opcov excludes from verdicts by
  design) and an Xdebug artifact on two data files. opcov's additional
  findings are the `??`/`?:` sides, `&&`/`||` short-circuits whose join block
  ran anyway, and loops never run zero or two or more times.
- **The frameless-call defect is gone.** The previous run (opcov built from
  fdf74e8dc0ad) had four unseen `??` outcomes and one uncovered line that
  came from the copy of an unqualified frameless call's arguments in its
  never-taken `JMP_FRAMELESS` fallback (same root as
  [#34](https://github.com/stubbedev/opcov/issues/34)). opcov now records
  the fallback copy's outcomes as twins of the frameless copy's and does not
  count lines only the fallback has
  ([how-it-works.md](how-it-works.md#compiling-a-file)), so those findings and
  their three explanations are gone and no row needs an opcov-defect entry.
- **Paths.** Xdebug enumerated thousands of paths through the changed
  functions and almost all of them were never taken; a path threshold on this
  branch would not be reachable.

## Migration notes

A traditional gate on Xdebug coverage combines a coverage threshold on the
changed code (lines, often branches, sometimes paths) with mutation testing
(Infection) for what coverage cannot see. With opcov, some of the mutation
rules become coverage rules, the coverage rules get stricter, and the paths
go away.

### Rules that become plain coverage checks

| Traditional rule | Why it needed mutation testing or paths | opcov finding |
|---|---|---|
| The fallback of `$a ?? $b` (and `??=`) ran | Xdebug compiles no branch for `COALESCE`: the line is covered as soon as `$a` is evaluated. Only a mutant that removes the fallback, surviving, showed that no test reached it. | unseen `CoalesceValue` / `CoalesceFallback` |
| Both sides of `$a ?: $b` ran | Same, for `JMP_SET`. | unseen `TernaryValue` / `TernaryFallback` |
| `$o?->m()` met `null` | Same, for `JMP_NULL`: a mutant replacing `?->` by `->` survives when no test passes `null`. | unseen `NullsafeNull` / `NullsafeValue` |
| A loop ran with no element and with several | Xdebug's branches only say the body ran; a loop exercised with exactly one element covers every block. Boundary mutants (empty iterable, `<` to `<=`) or path coverage (a path through the back edge) showed the gap. | loop never ran zero times (`zero`), never ran two or more times (`second`) |
| The implicit `else` of an `if` ran | php-code-coverage counts blocks: the block after the `if` runs whenever the condition is true too, so it is taken without the condition ever being false. Path coverage showed the missing path. | unseen `IfFalse` |
| `&&`/`||` short-circuited and evaluated | The join block after the condition runs on both outcomes, so block coverage cannot tell which one happened. | unseen `ShortCircuit` / `Evaluate` |

A path-coverage threshold has no opcov counterpart: opcov reports every
outcome (edge) and loop finding and never enumerates paths. Paths grow
exponentially with the decisions of a function (the comparison above shows
functions with thousands of paths), so a path threshold is rarely reachable;
the edges and loop findings above are the parts of it a test can be written
for. Replace a path threshold by "no unseen outcome and no loop finding in the
changed functions".

The executable lines differ in two documented places
([branch-semantics.md](branch-semantics.md#executable-lines)): a line that
only consumes a multi-line expression is covered only when one of its own
operands ran, and a file that only declares has no executable line.

### Rules that stay mutation-based

Coverage says a test ran the code and took the outcome; it does not say a test
would fail if the code were wrong. These stay with mutation testing:

- **Whether a test fails on the other outcome.** A test that takes
  `CoalesceFallback` but asserts nothing about the result passes with the
  fallback replaced. opcov makes the precondition visible (the outcome was
  taken, by which tests); whether the assertion catches a change is the
  mutant's question.
- **Boundaries and operators inside one outcome.** `<` versus `<=`, an
  off-by-one in an index, a wrong arithmetic operator or a negated return
  value change no control flow and produce no outcome of their own.
- **Removed calls and statements.** Coverage cannot tell that deleting a call
  would go unnoticed.

The gate output lists, for every executable line of a changed function, the
tests that ran it (`lines` in `--format json`). Feed those tests to the
mutation step (for example Infection's `--filter` on the changed files and a
test-framework filter on the listed tests), so mutation testing runs only the
mutants of the change against only the tests that reach them.
