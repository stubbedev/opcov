# Differential parity against PCOV and Xdebug

opcov must agree with PCOV and Xdebug on everything they can express, and every
difference must be explained. The parity suite runs the same PHP code under
opcov, PCOV and Xdebug (`XDEBUG_MODE=coverage` with branch checking),
normalizes the three results into one model and fails on any divergence that
`tests/diff/allowlist.json` does not explain, and on any allowlist entry that no
longer occurs.

## Running it

```sh
tools/measure/parity                                # golden CFG fixtures and tests/diff/fixtures
tools/measure/parity --suite corpus                 # the pinned real-world test suites
tools/measure/parity --suite corpus:carbon          # one corpus project
tools/measure/parity --suite golden --write-docs    # also rewrite this page's results for this PHP
OPCOV_MEASURE_PHP=php84 tools/measure/parity        # another PHP from the locked nixpkgs
```

Every PHP the harness starts gets an allowlisted environment (`PATH`,
`HOME`, `TMPDIR`, locale, `TZ`, the TLS certificate variables) plus the
variables it sets itself, never the caller's CGI, `PHP_INI_SCAN_DIR` or
Xdebug variables, and the harness refuses to start inside one of its own
measured children (`OPCOV_MEASURE` set). Each child has a time budget. On a
shared machine run the whole command under a memory cap, for example
`systemd-run --user --scope -p MemoryMax=6G -p TasksMax=500 tools/measure/parity`.

The script builds the measurement environment for the PHP version from the
flake's locked nixpkgs (`tools/measure/env.nix`: PHP, PCOV, Xdebug, turbocov,
composer, GNU time, perf, git, coreutils' `timeout`), builds opcov against that
PHP's `php-config` in `nix develop`, and runs the comparison. Set
`OPCOV_EXTENSION=/path/to/opcov.so` to measure an extension built elsewhere
(for example `nix build .#opcov-php85`). Everything it writes lives under
`target/measure-cache/`: the environment link, opcov's cargo target, the corpus
checkouts, each driver's normalized result and its log.

Exit status: `0` when every divergence is allowlisted and no allowlist entry is
stale, `1` when the comparison fails, `2` when the harness could not run (a
driver failed to load, a suite crashed, a budget ran out, a malformed
allowlist). Options: `--suite` (repeatable: `golden`, `fixtures`, `corpus`,
`corpus:<name>`; default `golden` and `fixtures`), `--budget <seconds>` per
driver run (default 3600), `--write-docs`.

## What runs

- **Golden CFG fixtures**: the PHP source embedded in every
  `crates/opcov-core/tests/golden/cfg/*.json`, called by
  `tests/diff/golden/<name>.php` with inputs that leave some outcomes untaken,
  with the expected values of opcov's own splits in
  `tests/diff/golden/<name>.expect.json`. A golden fixture without a runner or
  expectations, or a runner without a fixture, fails the run.
- **Construct fixtures** (`tests/diff/fixtures/<name>/src` measured,
  `run.php` returns the tests as closures, `expect.json` holds the expected
  values of opcov's own splits): generators and `yield from`,
  closures, arrow functions, static closures and first-class callables,
  property hooks, enums with interfaces and constants, `match` (chains, jump
  tables, multi-line arms, no arm), nullsafe chains (single- and multi-line),
  `try`/`catch`/`finally` with `return` inside `finally` and nested `try`,
  `foreach` over arrays and a user `Iterator`, `break 2`/`continue 2`,
  `while`, `do`-`while`, `goto`, `??`, `??=`, `?:`, `&&`/`||`/`and`/`xor`,
  multi-line conditions, `switch` on strings and `switch (true)`, traits,
  abstract classes, readonly classes, static variables, anonymous classes, a
  declarations-only file, heredoc and multi-line expressions.
- **Corpus** (`tools/measure/corpus.json`): the PHPUnit or Pest suites of
  Carbon 3.15.1, symfony/string v8.1.10 and spatie/laravel-query-builder 7.3.9
  (Laravel 12 through Orchestra Testbench, in-memory SQLite), each at a pinned
  commit (a shallow `git fetch` of that commit, so the commit hash pins the
  content) with dependencies installed from the lock stored in
  `tools/measure/corpus/<name>.lock`. `tools/measure/measure lock` regenerates
  the locks. A PHPUnit extension (`tools/measure/php/src/PhpUnitExtension.php`,
  added to a derived copy of the project's PHPUnit configuration) starts the
  driver at `Test\PreparationStarted` and stops it at `Test\Finished`, as
  PHPUnit's own coverage does, and the results of all tests are merged.
  Per project, `corpus.json` sets `xdebug` to `branch` (lines and outcomes
  compared) or `lines` (lines only):
  - symfony/string: `branch`, the full suite (2,932 tests).
  - Carbon: `lines`, the full suite (6,349 tests). Xdebug 3.5.3's branch mode
    crashes on it (SIGSEGV in `xdebug_path_add` called from
    `xdebug_branch_info_mark_reached`, under `iterator_to_array` over a user
    iterator, at a different test depending on the subset run), and it needed
    over an hour for the full suite before that.
  - spatie/laravel-query-builder is compared on lines only (`xdebug: lines`):
    with `XDEBUG_CC_BRANCH_CHECK`, Xdebug also enumerates paths, and on this
    suite one test ran for over 20 minutes at 100% CPU inside
    `xdebug_hash_extended_find`/`xdebug_hash_add_or_update` (perf), the path
    explosion of Xdebug's path coverage, which cannot be turned off
    separately. The suite assumes MySQL; on in-memory SQLite 54 of its 367
    tests fail their query-log assertions (identifier quoting), identically
    under every driver, while the code under test still runs.
  - Every project's own failures (Carbon's timezone and PHPStan tests on this
    machine) are the same under every driver; the harness accepts PHPUnit's
    exit codes 0 to 2 and requires every driver to run the same number of
    tests.

Every driver is called as its PHPUnit driver calls it: opcov `start()`,
`stop()`, `reset()`; PCOV `start()`, `stop()`, `waiting()`,
`collect(inclusive)`, `clear()`; Xdebug `xdebug_set_filter()` once, then
`xdebug_start_code_coverage(XDEBUG_CC_UNUSED | XDEBUG_CC_DEAD_CODE |
XDEBUG_CC_BRANCH_CHECK)`, `xdebug_get_code_coverage()`,
`xdebug_stop_code_coverage()`. Each driver runs in its own process with OPcache
off.

## The normalized model

Per file (relative to the measured root): every executable line with a hit
verdict; per function key the ordered list of **decisions** (a block or branch
with two or more successors), each with the line of its branching opcode and
its outcomes (the line the target starts on, the outcome kind for opcov, a hit
verdict or `null` when the driver does not report one); and per function key
opcov's **loops** (line, kind, "ran zero times" and "ran two or more times"
verdicts; `zero` is `null` for loops that cannot run zero times).

- PCOV and Xdebug lines: `1` covered, `-1` not covered, Xdebug's `-2` (dead
  code) is not executable.
- opcov lines: `\opcov\executable()` for the executable lines of every
  selected file, `\opcov\stop()` for the hits.
- Xdebug decisions: every branch with two or more distinct targets, in opcode
  order, targets in opcode order; edges to Xdebug's function-exit marker
  (`2147483645`) leave the function and have no opcov counterpart, and
  duplicate jump-table targets are one edge. Function names become opcov keys
  (`A->m` is `A::m`, `{main}` is `{main}@<file>`, `{closure:<file>:<a>-<b>}`
  is `{closure}@<file>:<a>-<b>`).
- opcov decisions: every block of `\opcov\get_debug_info()`'s CFG with two or
  more outgoing edges, with the edge kinds. Each edge is matched to the outcome
  of `\opcov\executable()` with the same site (the block's last opcode) and
  kind, and its verdict comes from the outcome ids `\opcov\stop()` returned.
  `Flow` edges (the "next case" edge of a `switch` chain, the fall-through into
  a `default`) are not outcomes, so their verdict is `null` and they are only
  compared structurally.
- opcov loops: `\opcov\executable()`'s loops, with the `zero`/`second` flags
  `\opcov\stop()` returned.

## How the comparison decides

**Lines.** Each line has one of three states per driver: not executable, not
covered, covered. When the three agree there is nothing to report. When PCOV
and Xdebug disagree with each other and opcov agrees with one of them, the line
is a *reference disagreement*: counted, not a failure, because opcov
cannot agree with both. Otherwise (opcov differs from both) it is a
divergence. opcov follows PCOV's executable-line rules
(`docs/configuration.md`, "Executable lines"), so most reference disagreements
are Xdebug's own rules (it counts the runtime `DECLARE_CLASS` line of a class
that extends or implements, and misses some multi-line nullsafe operands) or
PCOV's blind spots (it never sees property hooks).

**Outcomes.** opcov decisions whose outcomes are all `??`, `?:` or `?->`
splits are opcov-only (Xdebug does not branch on `COALESCE`, `JMP_SET` or
`JMP_NULL`): they are counted and checked against the fixture expectations
below, and left out of the alignment. Per function, the remaining opcov
decisions and Xdebug's decisions are aligned with a longest common
subsequence in which two decisions match when they sit on the same line and
have the same number of outcomes; outcomes pair up in opcode order of their
targets. A matched pair whose target lines differ is counted (Xdebug compiles
with `EXT_STMT`, so a target that is a `try` statement starts one line
earlier for Xdebug). `Engine` outcomes (the `JMP_FRAMELESS` check of an
unqualified call in a namespace) are seen by both drivers and compared like
any other. Then:

- a function that Xdebug has no branch data for (code compiled before Xdebug's
  coverage started, typically Composer `files` autoloads), and in which opcov
  saw no outcome either, is counted, not compared; an opcov hit in such a
  function is a divergence;
- an unmatched Xdebug decision with more than two targets whose first target
  is the line of an opcov decision is a `switch` jump table: opcov compiles
  measured files without jump tables (`docs/configuration.md`, "Jump tables"),
  so the same switch is opcov's chain of case decisions; counted;
- functions of anonymous classes are not compared, because Xdebug names every
  method of an anonymous class `class@anonymous` and loses which method it is;
  counted;
- every other unmatched decision on either side is a divergence;
- for matched decisions, every outcome whose verdict differs between opcov and
  Xdebug is a divergence.

**opcov's own splits.** Xdebug cannot express the two sides of `??`, `?:` and
`?->`, nor whether a loop ran zero or two or more times. For the fixtures,
every such opcov outcome and loop flag must match the value written in the
fixture's expectations (`{"outcomes": [{"function", "line", "kind", "hit"}],
"loops": [{"function", "line", "flag": "zero"|"second", "hit"}]}`, repeated
entries for several splits of one kind on one line, in opcode order). A
missing, extra or different value is a divergence of aspect `expected`. The
corpus has no expectations; its splits are only counted.

**Allowlist.** `tests/diff/allowlist.json` holds `rules` and `entries`. A
rule names a systematic, documented difference that the comparison recognizes
in code, with the reason and issue link that justify it:

- `declarations-only-return`: opcov reports no executable line where PCOV and
  Xdebug report a line at or after the file's last line, i.e. the constant
  `return` of a file whose main code only declares (ADR 0003,
  `docs/adr/0003-declarations-only-files.md`). Without this rule every class
  file of the corpus would need its own entry.

An entry `{"file", "line", "drivers", "aspect", "reason", "issue"}` explains
one divergence: `file` is
`<suite label>/<path>` as printed in the report (`fixtures/hooks/hooks.php`,
`corpus/carbon/src/Carbon/Carbon.php`), `aspect` is `lines` or `outcomes`,
`drivers` is the pair the divergence is reported for (`["opcov", "pcov"]` for
lines, `["opcov", "xdebug-branch"]` for outcomes and expectations), `reason`
explains it and `issue` links the GitHub issue. An entry whose suite ran but
that matched nothing, and a rule that matched nothing, fail the run as stale.

## Results

<!-- parity:php85:begin -->
PHP 8.5.12; opcov 0.1.0, pcov 1.0.12, xdebug 3.5.3.

|  | golden | fixtures |
|---|---|---|
| lines compared | 90 | 216 |
| lines where PCOV and Xdebug disagree | 0 | 13 |
| files loaded outside every test (only opcov reports them) | 0 | 0 |
| targets compared with Xdebug in line mode only | 0 | 0 |
| decisions matched to an Xdebug branch | 31 | 58 |
| ... of which Xdebug starts a target on an earlier statement line | 0 | 0 |
| functions without Xdebug branch data and without opcov hits | 0 | 6 |
| opcov-only decisions (??, ?:, ?->) | 5 | 14 |
| Xdebug switch jump tables (opcov: case chain) | 0 | 1 |
| decisions in anonymous classes (Xdebug cannot name) | 0 | 1 |
| outcome verdicts compared | 51 | 99 |
| outcome verdicts opcov does not report (Flow edges) | 12 | 21 |
| opcov-only outcomes and loop flags checked against fixture expectations | 21 | 39 |
| divergences | 18 | 15 |

Verdict: passed. 33 allowlisted, 0 unexplained divergences, 0 stale allowlist entries.

| rule | divergences | reason |
|---|---|---|
| `declarations-only-return` | 30 | A file whose main code only declares compiles to declaration opcodes and a constant `return`; opcov reports no executable line for it (ADR 0003, docs/adr/0003-declarations-only-files.md), PCOV and Xdebug report the `return` line at the end of the file. ([#13](https://github.com/stubbedev/opcov/issues/13)) |

| file:line | aspect | drivers | difference | reason |
|---|---|---|---|---|
| fixtures/hooks/hooks.php:17 | lines | opcov~pcov | opcov: not covered, pcov: not executable, xdebug-branch: not executable | A property hook that never ran is invisible to PCOV and Xdebug (neither walks property hooks); opcov reports its line as executable and not covered. ([#5](https://github.com/stubbedev/opcov/issues/5)) |
| fixtures/hooks/hooks.php:26 | lines | opcov~pcov | opcov: not covered, pcov: not executable, xdebug-branch: covered | Precision case: the unexecuted `default` arm of a multi-line match carries the expression's consuming opcode; Xdebug marks it covered, opcov does not probe consumer lines. PCOV does not see property hooks at all. ([#5](https://github.com/stubbedev/opcov/issues/5)) |
| fixtures/matches/matches.php:21 | lines | opcov~pcov | opcov: not covered, pcov: covered, xdebug-branch: covered | Precision case: the unexecuted last arm of a multi-line match shares its line with the `return` that consumes the match result; PCOV and Xdebug mark it covered whenever the match ran, opcov does not probe consumer lines. ([#5](https://github.com/stubbedev/opcov/issues/5)) |
<!-- parity:php85:end -->

<!-- parity:php84:begin -->
PHP 8.4.27; opcov 0.1.0, pcov 1.0.12, xdebug 3.5.3.

|  | golden | fixtures |
|---|---|---|
| lines compared | 90 | 215 |
| lines where PCOV and Xdebug disagree | 0 | 13 |
| files loaded outside every test (only opcov reports them) | 0 | 0 |
| targets compared with Xdebug in line mode only | 0 | 0 |
| decisions matched to an Xdebug branch | 31 | 58 |
| ... of which Xdebug starts a target on an earlier statement line | 0 | 0 |
| functions without Xdebug branch data and without opcov hits | 0 | 6 |
| opcov-only decisions (??, ?:, ?->) | 5 | 14 |
| Xdebug switch jump tables (opcov: case chain) | 0 | 1 |
| decisions in anonymous classes (Xdebug cannot name) | 0 | 1 |
| outcome verdicts compared | 51 | 99 |
| outcome verdicts opcov does not report (Flow edges) | 12 | 21 |
| opcov-only outcomes and loop flags checked against fixture expectations | 21 | 39 |
| divergences | 18 | 15 |

Verdict: passed. 33 allowlisted, 0 unexplained divergences, 0 stale allowlist entries.

| rule | divergences | reason |
|---|---|---|
| `declarations-only-return` | 30 | A file whose main code only declares compiles to declaration opcodes and a constant `return`; opcov reports no executable line for it (ADR 0003, docs/adr/0003-declarations-only-files.md), PCOV and Xdebug report the `return` line at the end of the file. ([#13](https://github.com/stubbedev/opcov/issues/13)) |

| file:line | aspect | drivers | difference | reason |
|---|---|---|---|---|
| fixtures/hooks/hooks.php:17 | lines | opcov~pcov | opcov: not covered, pcov: not executable, xdebug-branch: not executable | A property hook that never ran is invisible to PCOV and Xdebug (neither walks property hooks); opcov reports its line as executable and not covered. ([#5](https://github.com/stubbedev/opcov/issues/5)) |
| fixtures/hooks/hooks.php:26 | lines | opcov~pcov | opcov: not covered, pcov: not executable, xdebug-branch: covered | Precision case: the unexecuted `default` arm of a multi-line match carries the expression's consuming opcode; Xdebug marks it covered, opcov does not probe consumer lines. PCOV does not see property hooks at all. ([#5](https://github.com/stubbedev/opcov/issues/5)) |
| fixtures/matches/matches.php:21 | lines | opcov~pcov | opcov: not covered, pcov: covered, xdebug-branch: covered | Precision case: the unexecuted last arm of a multi-line match shares its line with the `return` that consumes the match result; PCOV and Xdebug mark it covered whenever the match ran, opcov does not probe consumer lines. ([#5](https://github.com/stubbedev/opcov/issues/5)) |
<!-- parity:php84:end -->

### Corpus, PHP 8.5.12

Run with `tools/measure/parity --suite corpus:<name>` against the extension
built from commit 022dfa0 plus the working tree of 2026-11-23 (debug profile).

| | symfony/string | Carbon | laravel-query-builder |
|---|---|---|---|
| Xdebug mode | branch | lines | lines |
| tests | 2,932 | 6,349 | 367 |
| lines compared | 1,356 | 10,876 | 782 |
| lines where PCOV and Xdebug disagree | 19 | 93 | 25 |
| decisions matched to an Xdebug branch | 741 | | |
| outcome verdicts compared with Xdebug | 1,471 | | |
| opcov-only decisions (`??`, `?:`, `?->`) | 65 | | |
| `declarations-only-return` (ADR 0003) | 8 | 38 | 42 |
| allowlisted precision cases | 0 | 12 | 0 |
| unexplained divergences | 5 | 0 | 0 |

The five unexplained divergences on symfony/string are one opcov bug: a
closure passed to an unqualified call of a frameless function inside a
namespace (`str_replace(' ', '', preg_replace_callback(..., static function
($m) {...}, ...))` in `AbstractUnicodeString::camel()`) appears twice in
opcov's model. PHP compiles the arguments of such a call twice, once for the
`JMP_FRAMELESS` fast path and once for the regular call, so the closure has
two op arrays; opcov names them `{closure}@AbstractUnicodeString.php:167-171`
and `...:167-171#1` (the ordinal meant for two closures on the same lines),
records the hits only on the copy that ran and reports the other copy's
outcomes as never taken, while Xdebug reports one function with the hits.
Minimal reproduction:

```php
<?php
namespace N;

function f(string $s): string
{
    return str_replace(' ', '', preg_replace_callback('/./', static function ($m) {
        return 'x';
    }, $s));
}
```

`\opcov\executable()` lists `{closure}@a.php:6-8` and `{closure}@a.php:6-8#1`;
without the namespace, or with a non-frameless outer call, it lists one. The
expected model has one function whose outcomes unite both copies (the copies
are the same source construct). The divergences stay unexplained until that is
fixed.

PHP 8.6 (8.6.0RC4 in the locked nixpkgs) has no parity run: neither PCOV
1.0.12 nor Xdebug 3.5.3 builds against it (`php86Extensions.pcov` and
`.xdebug` are marked broken in nixpkgs; PCOV fails on the removed
`INI_STR`/`INI_BOOL` macros), so there is nothing to compare against.

## CI

Add one check per PHP version (8.4 and 8.5) that runs, from a clean checkout:

```sh
OPCOV_MEASURE_PHP=php85 OPCOV_EXTENSION="$(nix build --print-out-paths .#opcov-php85)/lib/php/extensions/opcov.so" \
    tools/measure/parity --suite golden --suite fixtures
```

and fails on a non-zero exit. It needs network access only for the locked
nixpkgs and the turbocov source; it takes about a minute. The corpus
(`--suite corpus`) needs network access for git and composer and takes
about 40 minutes per PHP version with a release build of opcov (Xdebug's
branch mode on symfony/string takes about 10 of them); run it nightly, one
project at a time, with a memory cap (each project stays below 2 GB). As a `just` recipe:

```just
# Run the differential parity suite against PCOV and Xdebug.
parity *args:
    tools/measure/parity {{ args }}
```
