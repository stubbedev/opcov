# The `opcov` command-line tool

`opcov` reads the canonical opcov report ([report-schema.md](report-schema.md))
and merges, converts and checks it. Every subcommand prints `--help`.

| Subcommand | Purpose |
|---|---|
| `opcov merge` | Merge the reports of parallel workers into one report. |
| `opcov convert` | Write a report as lcov, Clover XML, Cobertura XML or HTML. |
| `opcov gate` | Check the functions a change touches: uncovered lines, unseen outcomes, loops. |
| `opcov impact` | From a baseline report, show the coverage of the functions a change touches and choose the tests that can reach them. |
| `opcov stitch` | Lay a run of the tests a change reaches over the baseline report, giving the report a run of every test would give. |
| `opcov consumers` | Find the code that uses changed PHP members, including the code the framework calls. |
| `opcov serve` | Keep a project's PHP index warm and up to date, so `opcov consumers` answers in milliseconds. |
| `opcov check` | Check a PHP change in one command: run the tests that reach it, keep the report current, and gate the changed code and its consumers. |
| `opcov doctor` | Check that a PHP binary can collect coverage with opcov, or with `--report` diagnose a report: outcomes that match no source construct. |

## Exit codes

| Code | `merge` | `convert` | `gate` | `impact` | `stitch` | `consumers` | `serve` | `check` | `doctor` | `doctor --report` |
|---|---|---|---|---|---|---|---|---|---|---|
| 0 | merged | converted | pass: no findings | the selection bounds the change | stitched | searched | stopped | pass, or nothing changed | PHP loads opcov and the probe self-test passed | every outcome that counts toward verdicts has a source construct |
| 1 | refused: the inputs do not merge (mismatching generator, settings, paths, file hash or model) | (not used) | findings | run every test: a change the baseline cannot bound | refused: a change the baseline cannot bound, a selected test the partial run lacks, or reports that do not merge | (not used) | (not used) | findings, a failing test command, or changed functions no test reaches | problems found | findings |
| 2 | could not run | could not run | could not run | could not run, or the baseline is not of the diff's base | could not run, the baseline is not of the diff's base, or the partial report is not of the working tree | could not run: no targets, an invalid request, no Composer autoload maps, or an error from `opcov serve` | could not run, or already running for the project | could not run, or the test command wrote no report | could not run | could not run |

"Could not run" covers usage errors, unreadable or invalid reports, missing or
unreadable inputs, a source file whose SHA-256 differs from the report's, a
source file that does not parse, and write errors. The message on standard
error names the input and the cause. Output files are written to a temporary
file next to the target and renamed into place, so a failed run never leaves a
partial file.

## Large reports

Every subcommand reads reports as a stream, one `files` entry at a time, and
never holds a whole report in memory, so memory does not grow with the number
of files:

- The reader splits the document into entries with a byte scanner and decodes
  each entry on its own; the header (`generator`, `settings`, `paths`,
  `tests`) is read once.
- `convert`, `gate` and `doctor` decode and process entries on a pool of
  worker threads, one per available core, while the next entries are read.
  At most 64 entries per worker are in flight; results are emitted in report
  order, so the output is byte-identical whatever the core count.
- Sources (`gate`, `doctor`, `convert --format html`) are read, hash-checked
  and parsed with mago on the same workers, so parsing runs on every core.
- `convert` to Clover, Cobertura and HTML reads the report twice: once for the
  totals their headers carry, once to write the files. Clover groups files by
  package, so the second pass spools the rendered `<file>` elements to an
  unlinked temporary file in the system temporary directory and copies them
  out per package.
- `gate` decodes only the entries of the changed files; the others are
  scanned and skipped.
- `merge` reads and decodes every input on its own thread, a few entries
  ahead of the merge.

Peak memory is a small multiple of the largest file entry times the worker
count, plus one small record per file for the two-pass formats. Measurements
are in [scale.md](scale.md#reports-and-cli).

## Source files

`gate`, `doctor` and `convert --format html` read the measured sources. A file
key of the report resolves against `paths.root` in relative mode, or is used
as is in absolute mode. The file's SHA-256 must equal the report's `sha256`;
otherwise the run stops with exit code 2, because the coverage belongs to a
different version of the file. Sources are parsed with `mago-syntax` through
`opcov-syntax`; a parse error is a hard error naming the file and position.

## `opcov merge`

```sh
opcov merge out.*.json -o out.json
```

See [Merging](report-schema.md#merging) for the semantics.

## `opcov convert`

```sh
opcov convert out.json --format lcov -o coverage.lcov
opcov convert out.json --format clover -o clover.xml
opcov convert out.json --format cobertura -o cobertura.xml
opcov convert out.json --format html -o coverage.html
```

| Flag | Meaning |
|---|---|
| `<REPORT>` | The opcov report to read. |
| `--format <clover\|cobertura\|lcov\|html>` | The format to write. |
| `-o, --output <OUT>` | The file to write. |

The output is a pure function of the report (plus, for HTML, the sources), so
the same report always converts to the same bytes. Timestamps that the formats
require are written as `0`, because the report records none.

### Common mapping

These rules apply to every format.

- **Counts are numbers of tests.** The report records which tests hit a line,
  not how often, so a line's count is the length of its hit list.
- **Branches.** Every outcome is one branch, except the outcomes that do not
  count toward verdicts (`EdgeKind::counts_toward_verdict`: `Engine`, and the
  `LoopEnter`/`LoopSkip` of `foreach`), which are left out. Every loop adds
  "ran two or more times" (taken by the tests in its `second` list) and, unless
  its `zero` is `null` (a loop that cannot run zero times), "ran zero times"
  (its `zero` list). A branch is
  covered when at least one test took it.
- **Functions.** A function's line range is its `span`, or for closures and
  arrow functions without a span the line range in their key. Each executable
  line belongs to the innermost function whose range contains it. A function
  counts as executed by the tests that executed any line it owns. A function
  with neither a span nor a line range in its key (for example `{main}` while
  the producer writes `span: null`) gets no function record; its lines and
  branches are still reported.
- **Classes.** Methods and property hooks belong to the class in their key.
  Closures and arrow functions belong to the class of the innermost method or
  hook that contains them. Everything else (functions, `{main}`, closures at
  file level) belongs to the file.
- **Names that are not UTF-8.** Paths and function keys may hold bytes that
  are not UTF-8, which reports encode as `%XX` (see "Bytes that are not
  UTF-8" in `docs/report-schema.md`). lcov writes `SF` as the raw path, so
  tools can open the file, and `FN`/`FNDA` names in their encoded form;
  Clover, Cobertura and HTML cannot hold invalid bytes and use the encoded
  form throughout.

### lcov

One record per file:

```
TN:
SF:/srv/app/app/Foo.php
FN:9,App\Foo::bar
FNDA:2,App\Foo::bar
FNF:1
FNH:1
BRDA:9,4,1,1
BRDA:13,5,2,0
BRF:2
BRH:1
DA:9,2
LF:1
LH:1
end_of_record
```

- `SF` is the absolute path (`paths.root` joined with the file key).
- `FN:<line>,<name>`: the name is the function key; the line is the
  function's first executable line, or its span start when it owns none
  (`genhtml` rejects function records on lines without `DA`).
- `FNDA:<tests>,<name>`; `FNF`/`FNH` count functions with a record.
- `BRDA:<line>,<block>,<branch>,<taken>`, numbered from the report's ids so the
  numbering is stable across runs:
  - functions are numbered by their position `i` in the file's `functions`
    (0-based, in report order);
  - outcome `o<n>` of function `i` is block `2i`, branch `n`;
  - loop `l<n>` of function `i` is block `2i + 1`, with branch `2n` for "ran
    zero times" and `2n + 1` for "ran two or more times";
  - `<taken>` is the number of tests that took the branch, or `-` when the
    branch was never taken and its line never ran.
- `DA:<line>,<tests>` for every executable line; `LF`/`LH` count them.

### Clover XML

opcov writes the Clover dialect of PHP's php-code-coverage, which Codecov,
Coveralls and SonarQube read:

```xml
<coverage clover="0.1.0" generated="0">
  <project timestamp="0">
    <metrics packages="1" files="1" classes="1" .../>
    <package name="App">
      <metrics files="1" classes="1" .../>
      <file name="/srv/app/app/Foo.php" path="/srv/app/app/Foo.php">
        <metrics classes="1" .../>
        <class name="App\Foo" namespace="App"><metrics .../></class>
        <line num="7" type="method" name="bar" complexity="0" count="2"/>
        <line num="11" type="stmt" count="1"/>
        <line num="13" type="cond" count="1" truecount="1" falsecount="3"/>
      </file>
    </package>
  </project>
</coverage>
```

- A file goes into the package named after the namespace of its first named
  class; files without a namespaced class sit directly in `project`.
- `line type="method"` sits on the function's span start line with the number
  of tests that executed the function. As in php-code-coverage, it replaces the
  statement entry of that line.
- `line type="cond"` is a line with branches: `truecount` is the number of its
  branches that were taken, `falsecount` the number never taken. Lines without
  branches are `type="stmt"`. `count` is the number of tests that ran the line.
- `metrics`: `statements` are executable lines, `conditionals` branches,
  `methods` functions with a record, `elements` their sum; the `covered*`
  attributes count the covered ones. `complexity` is not computed and is `0`.
- Validates against `crates/opcov-cli/tests/schemas/clover-php.xsd`, OpenClover's
  schema relaxed for the PHP dialect (see the README next to it).

### Cobertura XML

Cobertura 04 DTD structure, laid out like php-code-coverage's Cobertura report:

- `sources/source` is `paths.root` (absent in absolute mode); `filename`
  attributes are file keys.
- One `package` per file, named by its key, holding one `class` per class unit
  (see "Classes" above); the file-level unit is a class named by the file key.
- `method` elements for the functions with a record, `signature=""`, each with
  the lines it owns; `class/lines` holds every line of the class.
- `line hits` is the number of tests; a line with branches has `branch="true"`
  and `condition-coverage="<percent>% (<covered>/<total>)"`, the percent
  truncated to an integer.
- `line-rate` and `branch-rate` are covered/total truncated to four decimals,
  `1` when there is nothing to cover. `complexity` is not computed and is `0`.
- Validates against Cobertura's `coverage-04.dtd` (fetched from upstream when
  validating; not vendored, since Cobertura is GPL-2.0).

### HTML

A single self-contained page: a summary table (lines, branches and functions
per file) and every source file with each line's number of tests and covered
branches, coloured covered, uncovered or partially covered. It reads every
source file (hash-checked), so it needs the sources at `paths.root`. Bytes that
are not UTF-8 render as U+FFFD.

### Not converted here

php-code-coverage's serialized `.cov` format needs PHP's serializer and is
produced by the PHP package (`opcov/phpunit`), not by this tool.

## `opcov gate`

```sh
git diff -U0 origin/main... | opcov gate --report out.json --diff -
opcov gate --report out.json --files @changed.txt --format json
opcov gate --report out.json --files app/Foo.php:10-20,31 app/Bar.php
```

| Flag | Meaning |
|---|---|
| `--report <REPORT>` | The opcov report to check. |
| `--diff <DIFF>` | A unified diff (any context size; `git diff -U0` is smallest), or `-` for standard input. |
| `--files <ENTRY>...` | Changed files as `path[:start-end[,start-end...]]`, or `@listfile` with one entry per line; `@-` reads the list from standard input. A bare path means the whole file; `:N` means line `N`. |
| `--strip <N>` | Leading path components to remove from diff paths, as `patch -p` (default 1, which removes git's `a/` and `b/`). |
| `--base <DIR>` | The directory changed paths are relative to (default: the current directory). They are resolved lexically against it and then made relative to the report's `paths.root`. |
| `--format <text\|json\|files>` | Output format (default `text`). `files` prints one `path:first-last` per consumer, for `opcov gate --files @-`. |

Exactly one of `--diff` and `--files` is required.

### Which functions are checked

- A changed line selects the innermost function (by source span) that contains
  it. A run of removed lines that the diff replaces by new lines selects only
  through the new lines. A run of removed lines with no new lines in its place
  (a pure deletion) selects the innermost function that contains the gap it
  left (a deletion between two methods selects neither method, only the file's
  `{main}`). A bare path selects every function of the file.
- A selected function also selects the closures and arrow functions nested in
  it, since they are part of its code.
- Function ranges come from parsing the current source with `opcov-syntax`.
- A changed file the report does not contain is handled as described in
  [Changed files the report does not contain](#changed-files-the-report-does-not-contain).
- **Callers of changed members.** Finding the consumers of a changed member
  needs a call graph, which opcov does not build. Pass the consumers' ranges as
  extra `--files` entries; they are checked like any other change.

### Findings per function

| Finding | Meaning |
|---|---|
| uncovered lines | executable lines the function owns that no test ran |
| unseen outcomes | outcomes no test took, except those that do not count toward verdicts (`Engine`, and the `LoopEnter`/`LoopSkip` of `foreach`) |
| loops never zero | loops no test ran zero times (never a loop whose `zero` is `null`) |
| loops never twice | loops no test ran two or more times |

The output also lists, for every executable line of the function, the tests
that ran it, so a mutation step can rerun only those. A function that the
source declares but the report lacks is marked "not in the report" and only
its lines are checked.

### Changed files the report does not contain

The extension measures the files PHP compiles during the run, so a changed file
that no test loaded is not in the report. The gate decides for each such file
whether the run would have measured it, from the report's `settings`:

| The changed file | Result |
|---|---|
| does not end in `.php` | not in the report, `not a .php file` |
| is outside the report's `paths.root` | not in the report, `outside the report's root` |
| does not exist | not in the report, `does not exist` (`--files` naming a deleted file; a diff never lists deleted files) |
| is not below a `collect_paths` entry, or is below an `exclude_paths` entry | not in the report, with the reason as the extension states it (`not in opcov.collect_paths`, `excluded by opcov.exclude_paths entry <entry>`) |
| is selected by the paths while the report has an `exclude_pattern` | not in the report: the pattern is a PCRE that only PHP evaluates, so the gate cannot tell whether it excludes the file |
| is selected | **a finding**: "never loaded by any test", with its changed lines and the gaps of removed lines, or the whole file for a bare path |

A file that no test loaded is one finding, whatever the number of its changed
lines: the report holds no executable lines for it, and the gate does not
guess them from the source. This matches php-code-coverage, which includes the
uncovered files of `<source>` by default, so an Xdebug-based gate fails on them
too.

Relative `collect_paths` and `exclude_paths` entries are resolved against
`--base` (the working directory of the test run, which the extension resolved
them against), and the changed file by its real path, as the extension does.
An entry that no longer resolves is an error (exit code 2).

Text output:

```
app/Foo.php
  App\Foo::bar (lines 7-17)
    unseen outcome o6 LoopSkip at line 13
    loop l1 (Foreach, line 13) never ran zero times
    loop l1 (Foreach, line 13) never ran two or more times
    line 9: T\A::one, T\B::two
app/New.php
  never loaded by any test: changed lines 3-5; lines removed before 9
not in the report: README.md (not a .php file)
gate: 4 finding(s) in 1 changed function(s) and 1 changed file(s) no test loaded
```

JSON output (one line):

```json
{"verdict":"findings","findings":4,"files":[{"path":"app/Foo.php","functions":[{
  "key":"App\\Foo::bar","span":[7,5,17,6],"in_report":true,"uncovered_lines":[],
  "unseen_outcomes":[{"id":"o6","kind":"LoopSkip","line":13,"span":null}],
  "loops_never_zero":[{"id":"l1","kind":"Foreach","line":13}],
  "loops_never_twice":[{"id":"l1","kind":"Foreach","line":13}],
  "lines":[{"line":9,"tests":["T\\A::one","T\\B::two"]}]}]}],
  "unloaded":[{"path":"app/New.php","changed":{"parts":{"lines":[3,4,5],"removed_before":[9]}}}],
  "unmeasured":[{"path":"README.md","reason":"not a .php file"}]}
```

`verdict` is `pass` or `findings`; `kind` uses the report's outcome and loop
kind spelling. `findings` counts every function finding plus one per
`unloaded` file. An `unloaded` file's `changed` is `"whole_file"` or `parts`:
its changed lines, and the lines that runs of removed lines stood before.

### Diff input

The parser reads `git diff` output and plain unified diffs: file headers
`--- old` / `+++ new` (paths may be C-quoted by git; a tab and timestamp after
the path are ignored), hunks `@@ -a[,b] +c[,d] @@` and their `+`, `-`, ` ` and
`\ No newline at end of file` lines, git's `new file mode`, `deleted file
mode`, `rename from`/`rename to`, `copy to` and `Binary files ... differ`
lines. New files (`--- /dev/null`) change every line; deleted files (`+++
/dev/null`) change nothing; renamed files are checked under their new path;
pure renames, mode changes and binary files have no hunks and change nothing.
Other lines (`index`, `similarity index`, ...) are skipped. A hunk before a
`+++` header, a malformed hunk header, a line that does not fit the hunk's
counts and a diff that ends inside a hunk are errors naming the line.

## `opcov impact`

```sh
git diff -U0 "$BASELINE_COMMIT" | opcov impact --report baseline.json --diff -
opcov impact --report baseline.json --diff branch.diff --format json
vendor/bin/phpunit --filter "$(opcov impact --report baseline.json --diff branch.diff --format filter)"
```

`impact` answers two questions from a baseline report, without running a
test: what coverage the functions a change touches had, and which tests must
run to measure them again. The baseline is a report of the commit the diff
starts from, typically the last run on `main`; the diff goes from that commit
to the working tree.

| Option | Meaning |
|---|---|
| `--report <REPORT>` | The baseline report. |
| `--diff <DIFF>` | A unified diff from the baseline's commit to the working tree, or `-` for standard input. Any context size works; `git diff -U0` is smallest. |
| `--strip <N>` | Leading path components to remove from diff paths, as for `gate` (default 1). |
| `--base <DIR>` | The directory changed paths are relative to, as for `gate`. |
| `--ignore <PATH>` | A file or directory, relative to the base, whose changes cannot affect a test, such as `docs` or `README.md`. Repeatable. |
| `--whole-files` | Also select every test that ran a changed file, the selection [`stitch`](#opcov-stitch) needs. |
| `--format <text\|json\|filter>` | Output format (default `text`). `filter` prints a PHPUnit/Pest `--filter` expression for the selection. |

### Why tests can be skipped

A test's run on the changed code can only differ from its run on the
baseline's commit once it executes changed code: until then it runs the same
code with the same inputs. A test that never entered a changed function in the
baseline therefore cannot reach the change, and its coverage of the changed
functions is empty either way. The tests that did enter one are the
selection; running them and gating their report checks the change as a run of
the whole suite would.

### Checking the baseline

The baseline must be of the diff's base, file for file, or the selection is
wrong. `impact` checks it instead of trusting it:

- For every changed file, it rebuilds the base version from the working tree
  file and the diff (the diff holds the removed lines) and compares its
  SHA-256 with the baseline's. A context or added line that differs from the
  working tree is an error naming the line.
- Every other file of the baseline must still have its recorded SHA-256 on
  disk.

A mismatch exits 2 and lists the files; diff against the commit the baseline
was built from.

### What a change selects

| Change | Selection |
|---|---|
| Function bodies of a measured file, with nothing else changed outside them | the tests that hit a line, outcome or loop of each changed function in the baseline; functions are chosen from the base version by the rules of [`gate`](#which-functions-are-checked) |
| Only comments and whitespace outside function bodies | nothing |
| Any other change outside function bodies of a measured file: constants, properties, attributes, imports, doc comments, an added, removed or renamed method, top-level code | unbounded: coverage cannot tell which tests read a constant or reach a new override |
| A deleted or renamed measured file | every test that hit the file |
| A file whose every class is the class of a baseline test, with no other functions | those test classes, whole (`classes`) |
| A new file | its classes, whole, so new test classes run; unchanged code cannot reach a file that did not exist |
| A changed `.php` file below the collect paths that no baseline test loaded | nothing: no test reached it |
| Anything else: test helpers and base classes outside the collect paths, other files, binary files, files outside the report's root | unbounded, unless `--ignore` covers them |

Doc comments count as declarations because PHP code can read them through
reflection. A test class is recognised by name: the part of a test id before
`::`.

### Output

The text output lists every changed file with its effect. A changed function
shows its base lines, its baseline coverage (covered executable lines and
branch outcomes that count toward verdicts) and the tests that reached it. The
last line is the verdict: `impact: run N test(s) and M test class(es)`, or
`impact: run every test, ...` with exit code 1.

`--format json` writes one object: `verdict` (`bounded` or `unbounded`),
`tests` (the selected baseline tests, without those of selected classes),
`classes` (test classes to run whole) and `changes`, one entry per changed
file with `old`, `new` and an `effect` (`functions`, `whole_file`,
`test_classes`, `new_file`, `never_loaded`, `ignored`, `unbounded`) with its
details. With `--whole-files`, a change whose file is in the baseline also has
`file_tests`, the tests that ran the file, and they are part of `tests`.

`--format filter` prints a regular expression for PHPUnit's and Pest's
`--filter`, one alternative per class: the selected methods with every data
set, and whole classes. It prints nothing when no test needs to run (exit 0),
and for an unbounded change it prints nothing, writes the text output to
standard error and exits 1. Tests whose ids are not `Class::method` (`.phpt`
tests) cannot be selected by `--filter`; when the selection holds one, the
filter format exits 1 as well.

## `opcov stitch`

```sh
git diff -U0 "$BASELINE_COMMIT" > branch.diff
vendor/bin/opcov-phpunit vendor/bin/phpunit --filter "$(opcov impact --report baseline.json --diff branch.diff --whole-files --format filter)"
opcov stitch --baseline baseline.json --partial build/opcov.json --diff branch.diff -o branch.json
```

`stitch` writes the report a run of every test on the working tree would
write, from the baseline and a run of only the tests `impact --whole-files`
selects. The result serves everything a full report does: `convert`, coverage
totals, `gate`, and the baseline of the next change.

| Option | Meaning |
|---|---|
| `--baseline <REPORT>` | The baseline report: a run of every test on the commit the diff starts from. |
| `--partial <REPORT>` | The working tree's run of the selected tests. |
| `--diff <DIFF>`, `--strip <N>`, `--base <DIR>`, `--ignore <PATH>` | As for [`impact`](#opcov-impact). |
| `-o, --output <OUT>` | The stitched report to write. |

### Why the result is exact

A test that is not rerun never entered a changed function, so on the branch
it runs what it ran on the base ([Why tests can be skipped](#why-tests-can-be-skipped)):
its baseline hits are its branch hits. In unchanged files they sit at the same
lines, outcomes and loops; in a changed file they do not, since lines, spans
and outcome ids move. `--whole-files` therefore also selects every test that
ran a changed file, so no test that stays in the baseline has a hit in one.

The stitched report holds:

- **The test table:** the baseline's tests that were not rerun, and the partial
  run's tests. Rerun tests and the tests of selected classes leave the
  baseline, so a test the branch removed is gone.
- **Changed and new files:** the partial run's entry. A changed file that no
  rerun test loaded has no entry.
- **Unchanged files:** the baseline's entry without the rerun tests' hits,
  united with the partial run's entry. An unchanged file keeps its entry when
  only rerun tests loaded it and they no longer do; it then shows as
  uncovered.

### What it refuses

`stitch` evaluates the change like `impact --whole-files`, including the
baseline checks, and refuses rather than guesses:

- **A change the baseline cannot bound** (exit 1). Only a run of every test
  measures it.
- **A selected test missing from the partial report** (exit 1). The partial
  run was not the selection's run.
- **A baseline test with a hit in a changed file that is not rerun** (exit 1).
- **An unchanged file with another SHA-256 in the partial report** (exit 1).
- **Headers that differ** (exit 1): generator, settings or paths.
- **A baseline that is not of the diff's base** (exit 2).
- **A partial report whose files differ from the working tree** (exit 2).

Selected test classes are not checked against the partial report, since a
class the branch deleted has no tests to run. The partial report must
therefore be the run of the selection's filter, or a superset of it: extra
tests in the partial report replace their baseline data too.

The result assumes tests do not depend on what earlier tests left behind. A
test that only ran code memoised by an earlier rerun test, or that loaded a
file first, keeps the baseline's attribution of that code.

## `opcov consumers`

```sh
git diff -U0 origin/main... | opcov consumers --diff - --depth 2 --path app
opcov consumers --target 'method App\Actions\Publish::handle' --path app
opcov consumers --diff branch.diff --depth 2 --path app --format files | opcov gate --report build/opcov.json --files @-
```

`consumers` finds the code that uses changed members of a PHP project, so the
code that calls a change can be checked along with it. It analyses the project
in process with [mago](https://github.com/carthage-software/mago), without
running PHP.

| Option | Meaning |
|---|---|
| `--root <DIR>` | The project root: the directory with `composer.json` and `vendor/` (default: the current directory). |
| `--target <TARGET>` | A member to find the uses of (repeatable): `method Class::name`, `constant Class::NAME` (enum cases too), `property Class::name`, `function name` or `global-constant NAME`. |
| `--request <FILE>` | A JSON request, or `-` for standard input (see below). |
| `--path <PATH>` | Search only this directory or file, relative to the root (repeatable). The default is every source the root `composer.json` autoloads. |
| `--no-serve` | Search in this process even when `opcov serve` runs for the project. |
| `--diff <DIFF>` | A unified diff of the change, relative to the root (`git diff -U0`), or `-` for standard input. The members it changes are targets. |
| `--strip <N>` | Leading path components to remove from diff paths (default 1, which removes git's `a/` and `b/`). |
| `--depth <N>` | Call-graph levels to follow (default 1). At depth 2, the code that uses the consumers is found too. |
| `--format <text\|json\|files>` | Output format (default `text`). `files` prints one `path:first-last` per consumer, for `opcov gate --files @-`. |

### Which code is indexed

The project's own sources are the paths in the `autoload` section of the root
`composer.json`. Vendor code is indexed only where those sources reach it:
`consumers` follows every class the code and its docblocks name through
Composer's generated autoload maps (`vendor/composer/autoload_*.php`), so a
large `vendor/` costs only the parts the application uses. Run
`composer install` (or `composer dump-autoload`) first; without the maps
`consumers` cannot run.

### What a use is

- **A member use is matched by the receiver's type.** `$report->render()`
  reaches `Report::render`. A same-named method of an unrelated class does
  not, and neither does an override in a subclass.
- **Supertypes match:** a call through a supertype or interface the target
  implements matches, since dispatch can land on the target.
- **Unknown receivers match by name:** a receiver whose type is unknown
  (`mixed`, `object`) matches. A missed use is worse than an extra one.
- **Traits are analysed per using class.** A use the using class decides
  (`(new static())->handle()` in a trait) is reported as
  `<target> via <class>`; any other use belongs to the trait.
- **Code the framework calls is used where it is triggered.** `consumers`
  reads the hooks from the code itself:
  - a queued job's `handle` and `failed` where the job is created or
    dispatched;
  - an event subscriber's method where its event is created;
  - a model event handler by the calls that fire the event. That covers an
    observer registered with `Model::observe()` or `#[ObservedBy]`, a
    `static::<event>()` closure registered in a model's or trait's method,
    and a subscriber of an event in `$dispatchesEvents`. A trait as the
    model stands for every class that uses it;
  - a console command's `handle` where it is run by its name or class. The
    name comes from `#[AsCommand]`, `$signature` or `$name`.

### Laravel

Laravel resolves much of its API at runtime. `consumers` declares the facts it
needs to the analyzer, so chains keep their types instead of becoming `mixed`:

- **Model builders:** a model's builder is the class its `newEloquentBuilder()`
  returns, or `Builder<Model>`. Static calls on the model forward to that
  builder and to the query builder, and `query()` returns it.
- **Scopes:** `scopeX` and `#[Scope]` methods become builder methods.
- **Dynamic wheres:** `where{Column}()` returns the builder.
- **Relations:** a relation method with a generic `@return` (`HasMany<Post, $this>`) gives
  the model a typed property: the related model, or a collection of it.
- **Relation chains:** calls on a relation reach the related model's builder.
- **Facades:** a facade resolves to the class its accessor is bound to in a
  service provider (`bind`, `singleton`, `scoped`, `instance`).
- **Aliases:** the root aliases in `config/app.php` (`Artisan`, `DB`, …)
  resolve to their classes.

### Request

```json
{
    "targets": ["method App\\Jobs\\SendDigest::handle", "method App\\Observers\\PostObserver::created"],
    "within": ["app"]
}
```

`targets` is required. `within` limits the search like `--path`.

### Changed members and depth

A member is changed when a changed line falls inside it, or when lines were
deleted inside it. Members are methods, functions, class constants, enum
cases, properties (including promoted constructor parameters) and global
constants. Members of anonymous classes are not members of their own: their
lines belong to the enclosing function.

Each use belongs to its consumer: the innermost method or function around
it, or the line itself for code outside any function. A use in a trait that
the using class decides (`<target> via <class>`) belongs to that class's
copy of the trait method, so the next level looks for the callers of
`<class>::<method>`. Each level searches for the consumers the previous level
found, until `--depth` levels or no new consumer.

### Output

The text output lists the changed members and then the consumers, each as
`path:first-last`, the member, its depth and the targets it uses, then a
summary. `--format json` writes
`{"changed": [{"path", "first", "last", "target"}], "consumers": [{"depth", "path", "first", "last", "member", "uses"}], "analysed", "untyped": [{"path", "error"}], "index"}`;
`member` is null for code outside any function, and `served` says whether
`opcov serve` answered.

A file is analysed only when it names a target as a whole word, and each
file is analysed at most once however many levels search it; `analysed`
counts those files. When the analyzer fails on a file, its uses are matched
by name, as for a receiver of unknown type, and the file is listed under
`untyped` with the analyzer's error.

## `opcov serve`

```sh
opcov serve --root . &
git diff -U0 | opcov consumers --diff - --depth 2
opcov serve --stop
```

`opcov serve` indexes the project once, analyses every project file, and
then keeps the index up to date as files change. While it runs,
`opcov consumers` for the same root asks it instead of indexing the project
itself, so a search after an edit takes milliseconds instead of seconds. The
results are the same either way.

| Option | Meaning |
|---|---|
| `--root <DIR>` | The project root (default: the current directory). |
| `--stop` | Stop the server running for the project. |
| `--idle-timeout <SECONDS>` | Stop after this many seconds without a query. By default the server runs until stopped. |

**Staying current.** The server watches the autoloaded source directories,
`composer.json`, `composer.lock`, `config/` and `vendor/composer/`. Each
`opcov consumers` call also hands it the files its diff names, so a search
right after a save sees the save. An edit inside method or function bodies
updates the index in place: the file's declarations are replaced and only
its own facts are recomputed. A change to a declaration, a new or deleted
file, a file whose body feeds the framework facts (facades, service
providers, Eloquent builders and relations), a Composer change or an
`config/app.php` change rebuilds the index. The server logs each update and
rebuild with its duration on standard error.

**The socket.** The server listens on a Unix socket in `$XDG_RUNTIME_DIR`,
named after a digest of the project root, or in `<root>/.opcov/` when
`XDG_RUNTIME_DIR` is unset. Add `.opcov/` to `.gitignore` in that case. A
client of another opcov version is refused with a message to restart the
server.

## `opcov check`

```sh
opcov check --report build/opcov.json -- vendor/bin/opcov-phpunit vendor/bin/phpunit
opcov check --report build/opcov.json -- docker compose exec -T php vendor/bin/opcov-phpunit vendor/bin/phpunit
```

`opcov check` checks the change since the previous check in one command,
without git:

1. It finds the change by comparing the project's PHP files (the
   `autoload` and `autoload-dev` sources of `composer.json`) with its
   snapshot of them, taken when the report was last written.
2. It finds the changed members and their consumers to `--depth`, through
   [`opcov serve`](#opcov-serve). When no server runs for the project, the
   check starts one in the background with a one-hour idle timeout, so the
   next check finds the index warm.
3. It selects the tests that reach the change from the report, as
   [`impact --whole-files`](#opcov-impact) does, and runs the test command
   with `--filter <selection>` appended. When the report cannot bound the
   change it runs every test; when no test reaches it, it runs none.
4. It lays the run over the report, as [`stitch`](#opcov-stitch) does, so
   the report stays the report of a run of every test and is the baseline
   of the next check.
5. It gates the changed functions and the consumers' line ranges against
   the report and prints the verdict.

The first check, or one after the report was written by something else,
runs every test and takes the snapshot. A check with no change returns at
once. The test command runs in the root, with its output on standard error,
and must write the report given with `--report` (the opcov PHPUnit
extension's `output`). While it runs, the previous report is kept next to
it as `<report>.base` and put back if the run fails. The snapshot is kept
next to the report in `<report>.snapshot/`.

| Option | Meaning |
|---|---|
| `--report <REPORT>` | The report the test command writes, kept as the baseline between checks. |
| `--root <DIR>` | The project root (default: the current directory). |
| `--depth <N>` | Call-graph levels of consumers to gate (default 2). |
| `--ignore <PATH>` | A file or directory, relative to the root, whose changes cannot affect a test. Repeatable. |
| `--no-serve` | Search for consumers in this process instead of starting or asking `opcov serve`. |
| `-- <COMMAND>...` | The test command. |

## `opcov doctor`

```sh
opcov doctor
opcov doctor --php /usr/bin/php8.5 --format json
opcov doctor --report out.json
opcov doctor --report out.json --format json
```

| Flag | Meaning |
|---|---|
| `--php <BINARY>` | The PHP binary to check (default `php` from `PATH`). Not with `--report`. |
| `--report <REPORT>` | The opcov report to diagnose instead of checking PHP. |
| `--format <text\|json\|files>` | Output format (default `text`). `files` prints one `path:first-last` per consumer, for `opcov gate --files @-`. |

### Environment self-test

Without `--report`, doctor checks that the PHP binary can collect coverage
with this opcov. It writes a small probe file and a script to a fresh
temporary directory and runs the binary on them with `opcov.enabled=1` and
`opcov.collect_paths` set to that directory, with the binary's own INI files
(so the check sees the same `php.ini` and extensions as the test suite). The
child reports whether the extension loaded, its version, the PHP version,
thread safety, SAPI, the loaded INI files and `opcov\get_status()`, and runs
the probe: `start()`, one call through an `if`, `stop()`, and
`executable()` of the probe file. The self-test passes when exactly the
expected lines are covered and executable and the branch reports one outcome.
The child is stopped after 60 seconds and the directory is removed.

Each problem is reported with a code, the problem, the fix and a link to the
matching entry of [troubleshooting.md](troubleshooting.md):

| Code | Problem |
|---|---|
| `extension-not-loaded` | the binary does not load opcov |
| `build-mismatch` | opcov was built for another PHP minor version or another thread-safety (ZTS/NTS) or debug build; recognised from the loader's message |
| `version-mismatch` | the extension and the CLI come from different opcov releases |
| the code of a startup refusal or conflict | opcov loaded but refuses to collect (for example an unsupported OPcache or JIT setting), with the extension's own fix |
| `self-test-failed` | the probe covered the wrong lines or outcomes, or threw |

Anything PHP printed while starting (warnings, failed `extension=` lines) is
shown under "PHP printed while starting". Text output:

```
opcov doctor: php
  PHP 8.5.13 NTS (cli), opcov 0.1.0, VM HYBRID
  php.ini: /etc/php/php.ini
  probe self-test: passed
doctor: 0 problem(s)
```

JSON output is one object with `php_binary`, `runtime` (`php`, `zts`, `sapi`,
`opcov`, `vm_kind`, `ini_file`, `scanned_files`; `null` when the extension did
not load), `self_test` (`passed`, `not_run` or `failed`), `findings` (`code`,
`problem`, `fix`, `link`) and `startup_output`.

### Diagnosing a report

With `--report`, for every file of the report, doctor parses the source and matches every
outcome of every function to the source construct that produced it, with
`opcov-syntax`'s matcher (by line and construct kind; outcomes with the same
`site` belong to one construct, and the constructs of one kind on a line are
matched to the distinct sites on it in source order). It lists:

- outcomes that match no construct, with the reason (no construct of that kind
  on the line, more opcodes than candidate constructs, a construct claimed from
  several lines, a construct that cannot produce the outcome, an arm that does
  not exist, or an engine-internal outcome);
- report functions the source does not declare.

Exit code 1 when a function is missing or an unmatched outcome counts toward
verdicts; unmatched `Engine` outcomes are listed as "excluded from verdicts"
and do not fail the run.

Every outcome records its `site`, the index of the opcode it came from, so
doctor groups outcomes by site and matches each group to one construct.

Text output:

```
app/Foo.php
  App\Foo::bar o8 IfTrue on line 14: no source construct of this kind can produce an opcode on this line
doctor: 1 of 8 outcome(s) have no source construct
```
