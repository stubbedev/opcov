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
  "settings": {"collect_paths": ["app"], "exclude_paths": [], "exclude_pattern": null, "strict": true},
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
            {"id": "o7", "kind": "CoalesceFallback", "line": 14, "span": [14, 20, 14, 31], "hits": [0]},
            {"id": "o8", "kind": {"MatchArm": 2}, "line": 15, "span": null, "hits": []}
          ],
          "loops": [
            {"id": "l1", "kind": "Foreach", "line": 18, "zero": [], "second": [1]}
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
| `vm` | `"CALL"`, `"SWITCH"`, `"GOTO"` or `"HYBRID"` | The Zend VM kind PHP was built with. |

Outcome ids are derived from the opcodes PHP compiled, which depend on the PHP
build and the opcov version, so reports with different generators are never
merged.

### `settings`

| Member | Type | Meaning |
|---|---|---|
| `collect_paths` | array of strings | `opcov.collect_paths`, as a sorted set. |
| `exclude_paths` | array of strings | `opcov.exclude_paths`, as a sorted set. |
| `exclude_pattern` | string or `null` | `opcov.exclude_pattern`, or `null` when unset. |
| `strict` | boolean | The collector's strict flag, recorded as the producer set it. |

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
| `sha256` | string | SHA-256 of the measured source file, 64 lowercase hex digits, so a report never applies to a different version of the file. |
| `lines` | object | Executable lines, keyed by line number (a decimal string without leading zeros), each mapped to the hit list of the tests that executed it. |
| `functions` | object | Functions of the file, keyed by their stable function key: `Class::method`, `function`, `Class::$property::get` or `::set`, `{main}@<file>`, `{closure}@<file>:<start>-<end>`, `{fn}@<file>:<start>-<end>`, with `class@anonymous@<file>:<start>-<end>` as the class of an anonymous class. Keys are parsed into opcov's typed function key, so a malformed key is rejected. |

Every executable line, outcome and loop is present, including the ones nothing
hit: an empty hit list means "uncovered".

### `files.<path>.functions.<key>`

| Member | Type | Meaning |
|---|---|---|
| `span` | span or `null` | Source range of the function, or `null` when it is not known. |
| `outcomes` | array | Branch outcomes of the function, sorted by id. |
| `loops` | array | Loops of the function, sorted by id. |

### Outcome

| Member | Type | Meaning |
|---|---|---|
| `id` | string | `o` followed by a decimal number without leading zeros, unique within the function. |
| `kind` | outcome kind | What the outcome is (see below). |
| `line` | integer | Line of the branching construct, from 1. |
| `span` | span or `null` | Exact source range of the construct, or `null` when it could not be matched unambiguously. When present it contains `line`. |
| `hits` | hit list | Tests that took this outcome. |

Outcome kinds are the strings `IfTrue`, `IfFalse`, `ShortCircuit`, `Evaluate`,
`CoalesceValue`, `CoalesceFallback`, `TernaryValue`, `TernaryFallback`,
`NullsafeNull`, `NullsafeValue`, `MatchNoArm`, `SwitchDefault`, `LoopEnter`,
`LoopSkip`, `LoopBack`, `LoopExit`, `Catch`, `Finally`, `Engine` and `Jump`, or
one of the objects `{"MatchArm": n}` and `{"SwitchCase": n}` for the n-th
(zero-based) arm or case. `Engine` outcomes come from engine-internal jumps and
are excluded from verdicts by default.

### Loop

| Member | Type | Meaning |
|---|---|---|
| `id` | string | `l` followed by a decimal number without leading zeros, unique within the function. |
| `kind` | `"Foreach"`, `"For"`, `"While"` or `"DoWhile"` | The loop construct. |
| `line` | integer | Line of the loop header, from 1. |
| `zero` | hit list | Tests in which the loop ran zero times. |
| `second` | hit list | Tests in which the loop ran two or more times. |

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
  hooks, closures, arrow functions, each by its parts);
- outcomes and loops strictly ascending by the number in their id;
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
   loops, an empty `zero` means "never ran zero times" and an empty `second`
   means "never ran two or more times".
4. Validate reports from outside opcov against the JSON Schema. The Rust model
   in `opcov-core` (`opcov_core::report::Report::read`) checks the schema and
   every canonical-form rule above.
