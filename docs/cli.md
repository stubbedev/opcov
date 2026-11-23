# The `opcov` command-line tool

`opcov` reads the canonical opcov report ([report-schema.md](report-schema.md))
and merges, converts and checks it. Every subcommand prints `--help`.

| Subcommand | Purpose |
|---|---|
| `opcov merge` | Merge the reports of parallel workers into one report. |
| `opcov convert` | Write a report as lcov, Clover XML, Cobertura XML or HTML. |
| `opcov gate` | Check the functions a change touches: uncovered lines, unseen outcomes, loops. |
| `opcov doctor` | Check that a PHP binary can collect coverage with opcov, or with `--report` diagnose a report: outcomes that match no source construct. |

## Exit codes

| Code | `merge` | `convert` | `gate` | `doctor` | `doctor --report` |
|---|---|---|---|---|---|
| 0 | merged | converted | pass: no findings | PHP loads opcov and the probe self-test passed | every outcome that counts toward verdicts has a source construct |
| 1 | refused: the inputs do not merge (mismatching generator, settings, paths, file hash or model) | (not used) | findings | problems found | findings |
| 2 | could not run | could not run | could not run | could not run | could not run |

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
| `--format <text\|json>` | Output format (default `text`). |

Exactly one of `--diff` and `--files` is required.

### Which functions are checked

- A changed line selects the innermost function (by source span) that contains
  it. Lines removed by the diff select the innermost function that contains the
  gap they left (a deletion between two methods selects neither method, only
  the file's `{main}`). A bare path selects every function of the file.
- A selected function also selects the closures and arrow functions nested in
  it, since they are part of its code.
- Function ranges come from parsing the current source with `opcov-syntax`.
- A changed file the report does not contain (not measured, outside the root,
  or not PHP) is listed under "not in the report" and is not a finding. The
  gate cannot tell a file that no test loaded from one outside the collect
  paths.
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

Text output:

```
app/Foo.php
  App\Foo::bar (lines 7-17)
    unseen outcome o6 LoopSkip at line 13
    loop l1 (Foreach, line 13) never ran zero times
    loop l1 (Foreach, line 13) never ran two or more times
    line 9: T\A::one, T\B::two
gate: 3 finding(s) in 1 changed function(s)
```

JSON output (one line):

```json
{"verdict":"findings","findings":3,"files":[{"path":"app/Foo.php","functions":[{
  "key":"App\\Foo::bar","span":[7,5,17,6],"in_report":true,"uncovered_lines":[],
  "unseen_outcomes":[{"id":"o6","kind":"LoopSkip","line":13,"span":null}],
  "loops_never_zero":[{"id":"l1","kind":"Foreach","line":13}],
  "loops_never_twice":[{"id":"l1","kind":"Foreach","line":13}],
  "lines":[{"line":9,"tests":["T\\A::one","T\\B::two"]}]}]}],"unmeasured":[]}
```

`verdict` is `pass` or `findings`; `kind` uses the report's outcome and loop
kind spelling.

### Diff input

The parser reads `git diff` output and plain unified diffs: file headers
`--- old` / `+++ new` (paths may be C-quoted by git; a tab and timestamp after
the path are ignored), hunks `@@ -a[,b] +c[,d] @@` and their `+`, `-`, ` ` and
`\ No newline at end of file` lines. New files (`--- /dev/null`) change every
line; deleted files (`+++ /dev/null`) change nothing; renamed files are checked
under their new path; pure renames, mode changes and binary files have no hunks
and change nothing. Other lines (`diff --git`, `index`, `rename from`, ...) are
skipped. A hunk before a `+++` header, a malformed hunk header, a line that
does not fit the hunk's counts and a diff that ends inside a hunk are errors
naming the line.

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
| `--format <text\|json>` | Output format (default `text`). |

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
