# opcov report schema, version 1

An opcov report is one JSON document that holds line, branch-outcome and loop
coverage for a test run, with every hit attributed to the tests that caused it.
The schema id is `opcov-report/1`. The JSON Schema (draft 2020-12) is
[`schema/opcov-report.v1.json`](../schema/opcov-report.v1.json).

## Example

```json
{
  "schema": "opcov-report/1",
  "generator": {"opcov": "1.0.0", "php": "8.5.12", "zts": true, "vm": "HYBRID"},
  "settings": {"collect_paths": ["app"], "exclude_paths": [], "exclude_pattern": null},
  "paths": {"mode": "relative", "root": "/srv/app"},
  "tests": ["Tests\\Unit\\FooTest::test_bar", "Tests\\Unit\\FooTest::test_baz"],
  "files": {
    "app/Foo.php": {
      "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
      "lines": {"12": [0, 1], "14": []},
      "functions": {
        "App\\Foo::bar": {
          "span": [10, 1, 30, 5],
          "outcomes": [
            {"id": "o7", "site": 21, "kind": "CoalesceFallback", "line": 14, "span": [14, 20, 14, 31], "hits": [0]},
            {"id": "o8", "site": 25, "kind": {"MatchArm": 2}, "line": 15, "span": null, "hits": []}
          ],
          "loops": [
            {"id": "l1", "site": 29, "kind": "Foreach", "line": 18, "zero": [], "second": [1]},
            {"id": "l2", "site": 33, "kind": "PostTest", "line": 20, "zero": null, "second": []}
          ]
        }
      }
    }
  }
}
```

opcov writes the same document compactly, without whitespace, followed by one
newline.

## Fields

### Top level

| Member | Type | Meaning |
|---|---|---|
| `schema` | string | Always `"opcov-report/1"` for this version. |
| `generator` | object | The opcov build and PHP runtime that produced the report. |
| `settings` | object | The collection settings in force for the run. |
| `paths` | object | How the keys of `files` are resolved. |
| `tests` | array of strings | The test table. Every hit list refers to tests by their index in this array. |
| `files` | object | Coverage per measured file, keyed by path. |

### `generator`

| Member | Type | Meaning |
|---|---|---|
| `opcov` | string | Version of the opcov extension that collected the data. |
| `php` | string | PHP version, as `PHP_VERSION` reports it. |
| `zts` | boolean | Whether PHP was built thread-safe. |
| `vm` | `"CALL"`, `"SWITCH"`, `"GOTO"`, `"HYBRID"` or `"TAILCALL"` | The Zend VM kind PHP was built with (`TAILCALL` is what clang builds of PHP 8.5 and later use). |

Outcome ids are derived from the opcodes PHP compiled, which depend on the PHP
build and the opcov version, so reports with different generators are never
merged.

### `settings`

| Member | Type | Meaning |
|---|---|---|
| `collect_paths` | array of strings | The entries of `opcov.collect_paths` as configured (a listfile is expanded to its entries; entries are not resolved), as a sorted set. |
| `exclude_paths` | array of strings | The entries of `opcov.exclude_paths`, recorded the same way. |
| `exclude_pattern` | string or `null` | `opcov.exclude_pattern`, or `null` when unset. |

`exclude_paths` and `exclude_pattern` are recorded next to `collect_paths`
because together they decide which files a run measures; runs whose selections
differ do not merge.

### `paths`

Either `{"mode": "relative", "root": "<absolute directory>"}` or
`{"mode": "absolute"}`.

- In `relative` mode, the default, every key of `files` is relative to `root`,
  and `root` is a normalized absolute path (no empty, `.` or `..` segment, no
  trailing `/`; `/` itself is allowed). Relative paths keep reports from
  different checkouts comparable.
- In `absolute` mode, which a producer uses only when asked to explicitly,
  every key of `files` is an absolute path and `root` is absent.

A path never has an empty, `.` or `..` segment. The separator is `/`.

### `tests`

The names of the tests that ran, for example the PHPUnit test id
`Tests\Unit\FooTest::test_bar`. Names are non-empty, unique and sorted
ascending by their UTF-8 bytes. A test index is a zero-based position in this
array and fits in an unsigned 32-bit integer.

### `files.<path>`

| Member | Type | Meaning |
|---|---|---|
| `sha256` | string | SHA-256 of the measured source as PHP compiled it, 64 lowercase hex digits, so a report never applies to a different version of the file. The extension computes it when it writes the report, not while compiling: at compile time it keeps only a 64-bit fingerprint of the compiled bytes, and `write_report()` reads each file again, checks that the fingerprint still matches and hashes it. A measured file changed or removed on disk between compilation and `write_report()` therefore fails the report with an error naming it; code compiled from a string (`eval()`) is hashed when it is compiled. |
| `lines` | object | Executable lines, keyed by line number (a decimal string without leading zeros), each mapped to the hit list of the tests that executed it. |
| `functions` | object | Functions of the file, keyed by their stable function key (see below). Keys are parsed into opcov's typed function key, so a malformed key is rejected. |

Every executable line, outcome and loop is present, including the ones nothing
hit: an empty hit list means "uncovered". The top-level code of a file that
declares only classes, functions and constants has no executable line (see
[configuration.md](configuration.md#executable-lines)).

### Function keys

| Key | Function |
|---|---|
| `{main}@<file>` | the top-level code of the file |
| `function` | a function, with its namespace: `App\Support\helper` |
| `Class::method` | a method, with the class's namespace |
| `Class::$property::get`, `Class::$property::set` | a property hook |
| `{closure}@<file>:<start>-<end>` | a closure or an arrow function declared on lines `<start>` to `<end>` |
| `class@anonymous@<file>:<start>-<end>` | in place of `Class`: an anonymous class declared on lines `<start>` to `<end>` |

- `<file>` is exactly the `files` key the function is listed under: relative to
  `paths.root` in `relative` mode, absolute in `absolute` mode. A key that names
  another file is rejected.
- Closures and arrow functions share the `{closure}` form: PHP compiles both to
  the same kind of function (`fn () => $x` and `function () { return $x; }`
  produce the same opcodes and flags), so nothing that reads compiled code can
  tell them apart.
- When several closures (or several anonymous classes) of one file share the
  same line span, all but the first carry `#<n>`, their rank in source order
  among them, counted from 0 and written only from 1:
  `{closure}@a.php:24-24` and `{closure}@a.php:24-24#1` for
  `[fn () => 1, fn () => 2]`, `class@anonymous@a.php:3-9#1::run` for the run
  method of the second anonymous class on lines 3-9. A nested declaration ranks
  after the one that contains it. `#0` and numbers with leading zeros are
  rejected.

### `files.<path>.functions.<key>`

| Member | Type | Meaning |
|---|---|---|
| `span` | span or `null` | Source range of the function, or `null` when it is not known. The extension writes `null`: it sees compiled code, which has no columns. Spans come from parsing the source (`opcov-syntax`) when a report is consumed, matched by function key. |
| `outcomes` | array | Branch outcomes of the function, sorted by id. |
| `loops` | array | Loops of the function, sorted by id. |

### Outcome

| Member | Type | Meaning |
|---|---|---|
| `id` | string | `o` followed by a decimal number without leading zeros, unique within the function. |
| `site` | integer | Index (from 0) of the opcode the outcome comes from within its function: the branching opcode, or for `FinallyException` the first opcode of the `finally` block. Outcomes with the same `site` are the outcomes of one source construct. |
| `kind` | outcome kind | What the outcome is (see below). |
| `line` | integer | Line of the branching construct, from 1. |
| `span` | span or `null` | Exact source range of the construct, or `null` when it could not be matched unambiguously. When present it contains `line`. |
| `hits` | hit list | Tests that took this outcome. |

Outcome ids are numbered from `o0` in the order of (`site`, successor): by
the opcode they come from, then by the block they lead to. Two constructs of
the same kind on one line (`$a ?? $b ?? $c`) therefore differ by `site`, and a
consumer groups outcomes into constructs by `site` instead of counting the
n-th outcome of a kind on a line.

Outcome kinds are the strings `IfTrue`, `IfFalse`, `ShortCircuit`, `Evaluate`,
`CoalesceValue`, `CoalesceFallback`, `TernaryValue`, `TernaryFallback`,
`NullsafeNull`, `NullsafeValue`, `MatchNoArm`, `SwitchDefault`, `LoopEnter`,
`LoopSkip`, `LoopBack`, `LoopExit`, `Catch`, `Finally`, `FinallyException`,
`Engine` and `Jump`, or one of the objects `{"MatchArm": n}` and
`{"SwitchCase": n}` for the n-th (zero-based) arm or case.

| Kind | Taken when |
|---|---|
| `IfTrue`, `IfFalse` | the condition of an `if`/`elseif` or of a full `a ? b : c` ternary was true or false; an `if` without `else` still has `IfFalse` |
| `ShortCircuit`, `Evaluate` | `&&`/`and` or `\|\|`/`or` decided by its left operand, or went on to evaluate the right one |
| `CoalesceValue`, `CoalesceFallback` | `??` or `??=` found a non-null value, or used its fallback |
| `TernaryValue`, `TernaryFallback` | `?:` kept its truthy left operand, or used its fallback |
| `NullsafeNull`, `NullsafeValue` | `?->` met `null` and short-circuited, or went on |
| `MatchArm(n)`, `MatchNoArm` | a `match` took arm `n` (the `default` arm included), or no arm matched (`UnhandledMatchError`) |
| `SwitchCase(n)`, `SwitchDefault` | a `switch` jumped to case `n`, or to `default` (or past the `switch` without one) |
| `LoopEnter`, `LoopSkip` | a `foreach` started iterating, or skipped its loop entirely because the subject is not iterable or an empty object or `Traversable`; an empty array iterates zero times through `LoopEnter` and `LoopExit` |
| `LoopBack`, `LoopExit` | a loop test (the `foreach` fetch, or the condition of `for`, `while`, `do-while`) entered the body, or left the loop |
| `Catch` | a `catch` clause caught the exception |
| `Finally` | a `finally` block was entered by normal flow (`try` finished, or `return`, `break` or `continue` left it) |
| `FinallyException` | a `finally` block was entered because an exception unwound the `try` |
| `Engine` | an engine-internal jump (`JMP_FRAMELESS`, `BIND_INIT_STATIC_OR_JMP`, `ASSERT_CHECK`, a jump table's fallthrough) |
| `Jump` | an edge opcov could not classify (reported by `opcov-cli doctor`) |

`Engine` outcomes, and the `LoopEnter`/`LoopSkip` outcomes of `foreach`, are
reported but excluded from verdicts (`EdgeKind::counts_toward_verdict`): whether
a `foreach` ran zero times is what its loop's `zero` list says, and arrays
never take `LoopSkip`.

### Loop

| Member | Type | Meaning |
|---|---|---|
| `id` | string | `l` followed by a decimal number without leading zeros, unique within the function; loops are numbered from `l0` in the order of their header block. |
| `site` | integer | Index of the opcode that tests the loop, in the function's op array, from 0: the `FE_FETCH_R`/`FE_FETCH_RW` of a `foreach`, the conditional jump of `for`, `while` and `do-while`. It is the `site` of the loop's `LoopBack` outcome, stable for a given compiled function, and anchors the loop's two findings as branches where a format needs an opcode (see [branch-semantics.md](branch-semantics.md#loop-findings-as-branches)). |
| `kind` | `"Foreach"`, `"PreTest"` or `"PostTest"` | The loop's shape in the compiled code: `foreach`; a loop that tests before the body (`for`, `while`); a loop that tests after it (`do-while`). Which source keyword it was comes from the source when a report is consumed (`opcov-syntax`). Loops built from `goto` have no shape and are not reported. |
| `line` | integer | Line of the loop header, from 1. |
| `zero` | hit list or `null` | Tests in which the loop ran zero times, or `null` when it cannot: a `do-while`, and a loop whose condition is a constant true (`while (true)`, `for (;;)`). |
| `second` | hit list | Tests in which one activation of the loop ran its body two or more times. |

An activation is one execution of the loop from its entry: a recursive call,
another call of the function or a later entry into the loop starts a new one,
and a generator's activation spans its resumptions. A `foreach` counts its
iterations from the iterator's position, so iterating an `Iterator` never
calls one of its methods on opcov's behalf.

### Span

`[line_start, col_start, line_end, col_end]`: four integers, all starting at 1.
Columns count bytes and the end column is exclusive, so the start always lies
strictly before the end.

### Hit list

An array of test indexes, sorted ascending without duplicates. Every index is
smaller than the length of `tests`.

## Canonical form

A valid report is in canonical form. Beyond what the JSON Schema can express,
readers check and producers must emit:

- top-level members in the order `schema`, `generator`, `settings`, `paths`,
  `tests`, `files`;
- `tests`, `collect_paths`, `exclude_paths` and every hit list strictly
  ascending;
- the keys of `files` strictly ascending by UTF-8 bytes, and the keys of
  `lines` strictly ascending by number;
- the keys of `functions` unique (any order is accepted; opcov writes them in
  the order of its typed function key: `{main}`, functions, methods, property
  hooks, closures, each by its parts);
- the `<file>` of every function key equal to the `files` key it is listed
  under;
- outcomes and loops strictly ascending by the number in their id (opcov
  numbers outcomes by `site`, so their sites never decrease);
- every test index within the test table;
- every `files` key relative in `relative` mode and absolute in `absolute` mode;
- an outcome's `line` within its `span`.

Writing a report is deterministic, so two reports with the same content are
byte-identical, and a report opcov wrote reads back and writes again to the
same bytes. `schema` comes first
so that a reader rejects an unknown version before reading anything else;
`files` comes last and sorted so that a merge can stream each input file by
file.

## Version policy

- The schema id is `opcov-report/<major>`; only the major version is recorded.
- An additive change keeps `/1`: a new optional member, a new outcome kind, a
  new VM kind or another new enum value. Every other change (removing or
  renaming a member, changing a type or a meaning, tightening a rule) bumps the
  major version, and `schema/opcov-report.v<major>.json` is added next to the
  old schema.
- Readers reject a report whose major version they do not know, with an error
  that names the version found and the version supported. opcov checks the
  version before reading any other member.
- opcov's own reader is strict: it rejects members and values it does not know,
  because it rewrites reports (merge) and must never drop data silently. Read a
  report with an opcov at least as new as the one that produced it. Read-only
  third-party consumers may ignore unknown members and enum values of a `/1`
  report.
- The schema version is independent of the opcov version.

## Consuming a report

1. Read `schema` and refuse anything but `opcov-report/1`.
2. Resolve each `files` key against `paths.root` (relative mode) and compare
   the file's current SHA-256 with `sha256`; on a mismatch the coverage of that
   file is stale.
3. For each line, outcome and loop, map the indexes of its hit lists through
   `tests` to get the tests that hit it. An empty list means uncovered; for
   loops, an empty `zero` means "never ran zero times" (`null`: it cannot) and
   an empty `second` means "never ran two or more times".
4. Validate reports from outside opcov against the JSON Schema. The Rust model
   in `opcov-core` (`opcov_core::report::Report::read`) checks the schema and
   every canonical-form rule above.

## Merging

`opcov merge <inputs>... -o <out>` combines the reports of parallel workers,
for example one `<out>.<TEST_TOKEN>.json` per paratest worker:

- The test tables are united, deduplicated and sorted, and every hit list is
  re-indexed into the merged table.
- Files, lines, functions, outcomes, loops and hit lists are united.
- The output is canonical, so it is byte-for-byte the same for any order of the
  inputs; merging is commutative, associative and idempotent.
- The merge refuses, with exit code 1: reports whose `generator`, `settings` or
  `paths` differ, naming the field and both values; a file whose `sha256`
  differs between inputs, naming the path; a function, outcome or loop whose
  model (span; site, kind, line; or for a loop site, kind, line or whether `zero` is `null`) differs between
  inputs; an unknown schema version.
- Invalid input, unreadable inputs and write errors exit with code 2. The
  output is written to a temporary file next to it and renamed into place, so a
  failed merge never leaves a partial report.

Memory: the merge reads every input twice. The first pass reads only the
headers, skipping the file entries without keeping them, and builds the merged
test table. The second pass streams all inputs at once in path order: it holds
one pending path per input and parses an input's next file entry only when the
merge consumes it. Peak memory is the merged test table plus one index per
input test, the largest merged file entry, one file entry being parsed, and per
input one reader thread with its read buffer and one path. It grows with the
number of inputs only through that per-input constant (one thread and one open
file per input), never with the size of the inputs. Inputs must not change
while a merge runs.
