# Configuration

opcov is configured with four INI settings. All of them are
`PHP_INI_SYSTEM | PHP_INI_PERDIR`: set them in `php.ini`, with `-d` on the
command line, or per directory where the SAPI supports it. `ini_set()` at run
time is refused (it returns `false`), because the file selection is decided
when PHP compiles a file.

| Setting | Default | Meaning |
|---|---|---|
| `opcov.enabled` | `0` | `1` turns opcov on. Loading the extension with `0` changes nothing. |
| `opcov.collect_paths` | empty | Files and directories to measure. Required when enabled. |
| `opcov.exclude_paths` | empty | Files and directories removed from the selection. |
| `opcov.exclude_pattern` | empty | A PCRE that removes matching files from the selection. |

## `opcov.enabled`

Read twice:

- at module startup: with `0`, opcov reserves no `op_array.reserved` slot,
  registers no `zend_extension` and never installs a hook. Collection is then
  refused for the whole process with
  `opcov.enabled was 0 when PHP started; set opcov.enabled=1 in php.ini or with -d`.
- at each request start: with `0` for that request, no file is measured and
  collection is refused with `opcov.enabled is 0`.

opcov installs its hooks at the first request with `opcov.enabled=1`, not at
module startup, so it wraps the `zend_compile_file` / `zend_compile_string`
hooks of extensions that started before it (OPcache) and code compiled before
that request (OPcache preloading) keeps its handlers.

## `opcov.collect_paths` and `opcov.exclude_paths`

Syntax, the same for both settings:

- a `:`-separated list of paths: `opcov.collect_paths=/app/src:/app/lib/helpers.php`;
- or `@` followed by the path of a listfile with one path per line:
  `opcov.collect_paths=@/tmp/phpunit-source.txt`. Blank lines and surrounding
  whitespace are ignored.

Semantics:

- Every entry is resolved once at request start with `realpath()`: symbolic
  links and `.`/`..` segments are resolved, relative entries are taken
  relative to the working directory at request start.
- An entry that is a file selects exactly that file. An entry that is a
  directory selects every file below it, matched by whole path components:
  `/app` selects `/app/src/a.php` but not `/application/a.php`.
- A compiled file is measured when it is below a collect entry, below no
  exclude entry and not matched by `opcov.exclude_pattern`.
- Paths are compared case-sensitively on Linux and case-insensitively on
  macOS.

Errors turn collection off for the request; `\opcov\ensure_can_collect()`,
`\opcov\start()` and `\opcov\stop()` then throw
`\opcov\CannotCollectException` with the message, and
`\opcov\get_status()['off_reason']` holds it. Nothing is printed at startup.

| Situation | Message |
|---|---|
| `opcov.collect_paths` empty | `opcov.collect_paths is not set` |
| an entry does not exist | `opcov.collect_paths: cannot resolve <entry>: <OS error>` |
| `@` without a file name | `opcov.collect_paths: ` + "`@` must be followed by the path of a listfile" |
| listfile missing or unreadable | `opcov.collect_paths: cannot read listfile <path>: <OS error>` |
| listfile without paths | `opcov.collect_paths: listfile <path> contains no paths` |
| a setting is not UTF-8 | `<setting> is not valid UTF-8` |

`opcov.exclude_paths` reports the same errors under its own name.

## `opcov.exclude_pattern`

A PCRE in PHP's `preg_*` syntax, with delimiters and modifiers, for example
`opcov.exclude_pattern=~/(tests|fixtures)/~`. It is compiled with PHP's own
PCRE (`pcre_get_compiled_regex_cache`) at request start and matched against
the resolved path of each compiled file (for `eval()`'d code: the resolved
path of the file that called `eval()`).

- An invalid pattern turns collection off with
  `opcov.exclude_pattern is not a valid PCRE pattern with delimiters: <pattern>`.
- A matching error at run time (backtracking or JIT stack limit) turns
  collection off with `opcov.exclude_pattern failed to match against <path>`.

## When the decision is made

The selection is decided once per compiled file name and cached for the
process. `\opcov\get_status()` lists the selected files (`selected_files`) and
every skipped file with the reason (`skipped_files`):

| Reason | Meaning |
|---|---|
| `not in opcov.collect_paths` | no collect entry contains the file |
| `excluded by opcov.exclude_paths entry <entry>` | an exclude entry contains it |
| `matched by opcov.exclude_pattern` | the pattern matched its resolved path |
| `loaded through a stream wrapper, which is not selectable by path` | see below |
| `its path cannot be resolved to an existing UTF-8 file name` | see below |

A file included under two paths (through a symlink, with `..`, or relative
and absolute) is one file: both resolve to the same real path, which is the
key of every report.

## Jump tables

`switch` statements with constant cases normally compile to `SWITCH_LONG` /
`SWITCH_STRING` jump tables, which skip the comparison of each case. Measured
files are compiled with `ZEND_COMPILE_NO_JUMPTABLES`, so every case is a
comparison and a jump that probes can see. The decision is made before
compilation from the file handle's opened path, or from `zend_resolve_path()`
of its name:

- a file that is not selected keeps its jump tables and runs at full speed
  (`get_status()['counters']['jump_table_files']` counts them);
- a file whose path cannot be resolved before compilation, and every
  `eval()`'d string, compiles without jump tables: this may lose speed, never
  coverage.

`match` is different: PHP compiles a `match` with constant arms to a `MATCH`
jump table whatever the compiler options say. Its arms are still separate
blocks and edges of the CFG, and the probe on the `MATCH` opcode looks the
subject up in the jump table itself to record the arm taken.

## `eval()` and stream wrappers

- `eval()`'d code is measured when the file that called `eval()` is selected
  (nested `eval()`s are traced back to the outermost file). Each distinct
  source evaluated at a call site is a file of its own, named after the
  engine's name for the code and the first 16 hex digits of the SHA-256 of
  the evaluated string:
  `<file>(<line>) : eval()'d code #<digest prefix>`, where `<file>` is the
  resolved path of the calling file. One call site that evaluates different
  code (mixins, macros, generated classes) therefore yields one entry per
  source, and the same source evaluated again, in this process or another,
  is the same entry. `stop()`, `executable()`, `get_status()` and
  `write_report()` all use this name. The `opcov` CLI's `convert` leaves
  these entries out (they have no source file to show or point to, as
  php-code-coverage leaves eval'd code out of its reports), `doctor` counts
  them without checking them, `merge` keeps them, and `gate` never selects
  them, since no changed file is eval'd code.
- Files loaded through a stream wrapper (`phar://`, `vfs://`, a userland
  wrapper) are not selectable by path. They run unmeasured and are listed as
  skipped; nothing fails.
- A file name that cannot be resolved to an existing file (for example
  `Command line code` of `php -r`) is skipped. Names that are not valid UTF-8
  are measured like any other; reports encode them losslessly (see
  `docs/report-schema.md`).

## Executable lines

The executable lines of a file follow PCOV's rules so that both report the
same lines: every opcode of a reachable block is executable except `NOP`,
`OP_DATA`, `FE_FREE`, `FREE`, `ASSERT_CHECK`, `VERIFY_RETURN_TYPE`, `RECV`,
`RECV_INIT`, `RECV_VARIADIC`, `SEND_VAL`, `SEND_VAR_EX`, `SEND_REF`,
`SEND_UNPACK`, `DECLARE_CONST`, `DECLARE_CLASS`, `DECLARE_CLASS_DELAYED`,
`DECLARE_FUNCTION`, `DECLARE_ANON_CLASS`, `FAST_RET`, `FAST_CALL`, `TICKS`,
`EXT_STMT`, `EXT_FCALL_BEGIN`, `EXT_FCALL_END`, `EXT_NOP` and `BIND_GLOBAL`;
the `DO_FCALL` of a `new` expression counts with its `NEW`; abstract methods
have no lines.

Each line gets one probe per entry into a block. A line that only consumes
the value of a multi-line expression (the `return` of `return $a\n || $b;`,
a `match` arm's result, the `: null` of a multi-line ternary) gets no probe,
so it is reported as covered only when one of its own operands ran. PCOV
reports such lines as covered whenever the expression ran; these are the
documented precision differences.

A file whose top-level code is only declarations (classes, interfaces, traits,
enums, functions, constants) has no executable line: its main code is the
declaration opcodes, which are never lines, and a final `RETURN 1`, which the
VM skips on `include` when nothing replaced `zend_execute_ex` and runs
otherwise. opcov reports no line for it, whatever the executor. PCOV and
Xdebug, which replace `zend_execute_ex`, report that `RETURN`'s line as
executed by whichever test happened to load the file first, so their per-test
data depends on test order and on how a run is split across processes; opcov's
does not, which keeps a merged parallel report identical to a serial one.

`\opcov\executable($files)` compiles nothing: it reports the model of files
that were compiled in this process and omits files that were never loaded,
so a runner must treat a missing file as "not loaded" (php-code-coverage's
uncovered-file handling reads such files itself).

## Collection API

All functions live in the `opcov` namespace.

| Function | Behaviour |
|---|---|
| `ensure_can_collect(): void` | Throws `\opcov\CannotCollectException` (a `RuntimeException`) with the reason when collection is off, or when the setup conflicts with probes (OPcache's optimizer, JIT, file cache or preload, OPcache under a non-CLI SAPI, Xdebug's or PCOV's coverage mode; the settings are read once per request, and the ones a script can change at run time, `opcache.enable` and `opcache.jit`, are read again on every call, so `ini_set('opcache.jit', ...)` is caught). The message ends with a link to its entry in [troubleshooting.md](troubleshooting.md). After at least one `start()`/`stop()` cycle it also throws when no compiled file was selected. |
| `start(): void` | Starts recording. Re-arms the probes that fired while collection was off (bootstrap code), so the collection sees them again. Throws `CannotCollectException` when collection is off and `LogicException` when already collecting. |
| `stop(?string $test = null): array` | Stops recording and returns what fired since `start()` (shape below), sorted by file, line, function key and id. It does not re-arm the probes, and it discards branch watches still pending, since their frames are gone once a test ends (or `exit()` or a fatal error unwound them). With `$test`, the same hits are also attributed to that test id for `write_report()`. Throws `LogicException` without a `start()`, `CannotCollectException` when collection went off meanwhile, and `ValueError` (before stopping) when `$test` is empty or not UTF-8. |
| `reset(): void` | Re-arms every probe that fired since the last `reset()`; its cost is proportional to the probes that fired, not to the probes installed. Throws `LogicException` while collecting. |
| `executable(array $files): array` | `[file => ['lines' => [line => []], 'functions' => [key => ['span' => null, 'outcomes' => [...], 'loops' => [...]]]]]` for the given files that were compiled, built by the same conversion as a report's `lines` and `functions` (function keys name the file by its resolved path; outcomes and loops are the report's, with empty hit lists). |
| `write_report(string $path, array $options = []): void` | Writes a canonical `opcov-report/1` report (see [report-schema.md](report-schema.md)) to `$path`; see below. |
| `get_status(): array` | The state of collection and everything that explains it; keys below. |
| `get_debug_info(string $file): array` | `skip_reason` (why the file is not measured, or `null`), `executable_lines`, `units` (live compiled copies of the file), `functions` (function key => CFG snapshot with blocks, edges with their kinds, loops and diagnostics) and `probes`: per probe slot `function`, `op` (opcode index), `line`, `roles` (the roles the slot has: `Line`, `Source`, `Watch`, `LoopEntry`, `FinallyEntry`), `armed` (the roles still armed), `watchers` (pending branch sources watching it), `source_outcomes` (the outcomes a `Source` decides), `watched_outcomes` (the outcomes it resolves as a successor), `loop_entries` and `finally_outcome`. Bug reports attach it. |

`get_status()` returns:

| Key | Value |
|---|---|
| `enabled` | `opcov.enabled` for this request |
| `collecting` | `true` between `start()` and `stop()` |
| `off_reason` | why collection is refused, with its troubleshooting link, or `null` |
| `off_code` | the [troubleshooting](troubleshooting.md) code of `off_reason`, or `null` |
| `owner_thread` | `current`, `other` or `unclaimed`: the thread that owns the probes (always `current` on NTS) |
| `zts` | whether PHP is thread-safe |
| `vm_kind` | `CALL`, `SWITCH`, `GOTO`, `HYBRID` or `TAILCALL` |
| `sapi` | the SAPI name, for example `cli` |
| `opcache` | `null` when OPcache is not loaded, else `active` (caching for this SAPI) and the settings opcov checks: `enable`, `enable_cli`, `protect_memory`, `file_cache`, `jit`, `jit_buffer_size`, `preload`, `optimization_level` |
| `conflicts` | every unsupported setup found now, as `code`, `problem` and `fix` (OPcache, JIT, Xdebug and PCOV coverage modes; see [support.md](support.md#opcache-and-jit) and [compatibility.md](compatibility.md)) |
| `selected_files` | the measured files compiled so far |
| `skipped_files` | compiled file => why it is not measured |
| `failed_files` | measured file whose model could not be built => `{code, reason}`; the file is left out and collection continues for every other file |
| `counters` | `files_selected`, `files_skipped`, `files_failed`, `units` (live compiled copies of measured code), `probes_installed`, `probes_fired`, `probes_rearmed` (re-arms by `start()`, `reset()` and request end), `watches_installed` (successor watches a branch source installed because it could not decide its outcome from the operands), `pending_discards` (watches dropped unresolved: an exception, `stop()`, a destroyed unit, or a frame that left), `jump_table_files` |

After `exit()` or a fatal error during collection, shutdown functions still
see a consistent opcov: `collecting` is still `true`, `stop($test)` attributes
the hits made up to the interruption to that test, and `write_report()` writes
a complete report (`tests/phpt/process/exit.phpt`, `fatal.phpt`). A fatal error
raised inside opcov's own engine calls is caught there and resumed after opcov's
frames returned (ADR 0001, "Fatal errors").

`stop()` returns, for every file with a hit:

```php
[
    '/srv/app/src/Foo.php' => [
        'lines' => [12 => true, 14 => true],
        'functions' => [
            'App\Foo::bar' => [
                'outcomes' => ['o0' => true, 'o3' => true],
                'loops' => ['l0' => ['zero' => true], 'l1' => ['zero' => true, 'second' => true]],
            ],
        ],
    ],
]
```

Files are keyed by their resolved path and functions by the function key the
report uses, with the file named by its resolved path. A function is listed
only when one of its outcomes or loops was hit; `lines` and `functions` are
always present. Outcome ids `o<n>` and loop ids `l<n>` are the ids of
`executable()` and the report, which name the kind, line and site of each
(see [report-schema.md](report-schema.md)); a loop lists `zero` when it ran zero
times and `second` when it ran two or more times in one activation.

A test runner calls `start()`, runs one test, calls `stop()` and then
`reset()`, so each test is measured on its own; without `reset()` a probe
that fired in one test stays fired in the next. A branch probe stays armed
until every outcome of its branch (and, for a loop test, both loop flags) was
seen since the last `reset()`, so a loop running a branch whose outcomes were
all seen runs at full speed.

If the request ends while collecting, the pending hits are discarded at
request shutdown.

### Per-test attribution and `write_report()`

A runner passes each test's id to `stop()` and writes one report at the end:

```php
foreach ($tests as $test) {
    \opcov\start();
    $test->run();
    \opcov\stop($test->id());
    \opcov\reset();
}
\opcov\write_report('build/coverage.json');
```

- Attribution accumulates in the extension for the life of the process: it
  survives `reset()`, a test id stopped twice is one test whose hits are
  united, and memory grows with the distinct (test, line, outcome or loop
  flag) pairs, stored as one sorted list per test. `stop()` without a test id
  attributes nothing.
- `write_report()` writes every attributed test and every measured file
  compiled in this process, including files no test touched: every executable
  line is present, with the sorted indexes of the tests that ran it, or an
  empty list, and every outcome and loop likewise. Spans (of functions and
  outcomes) are `null`: the extension sees compiled code, which has no
  columns; they come from the source when a report is consumed. It may be called more than once; each call writes the state so far.
- The report is serialised by `opcov-core`, the same writer as `opcov merge`,
  to a temporary file next to `$path` (`.<name>.<pid>.tmp`), synced and renamed
  into place, so `$path` is either the previous file or the complete report.
- `sha256` is computed once per file at compile time from the bytes PHP
  compiled (the file handle's buffer, or the `eval()`'d string). When OPCache
  serves a file from its cache PHP reads nothing, so opcov reads the file once
  at that point; if it changed on disk after OPCache cached it, the recorded
  hash is that of the new content. A file compiled twice from different bytes
  in one process (a rewritten file included again) has no single hash, and
  `write_report()` refuses it. `eval()`'d code cannot hit this: each distinct
  source is its own entry (see [`eval()` and stream
  wrappers](#eval-and-stream-wrappers)).

Options (`$options`):

| Key | Type | Default | Meaning |
|---|---|---|---|
| `root` | string | the working directory | Directory the `files` keys are relative to. Relative values are resolved against the working directory; symbolic links are resolved, because measured files are recorded by their real path. |
| `absolute_paths` | bool | `false` | Key `files` by absolute path and record no root; cannot be combined with `root`. |

Relative `$path` values are resolved against PHP's working directory.

| Situation | Exception |
|---|---|
| unknown option key, empty `$path` or `root`, `root` with `absolute_paths` | `ValueError` naming the argument |
| an option of the wrong type | `TypeError` naming the key |
| collection is off | `\opcov\CannotCollectException` |
| called while collecting | `LogicException` |
| a measured file outside `root`, a file compiled from different sources, the root cannot be resolved, the report cannot be written | `RuntimeException` naming the path and the reason |

## Processes and threads

- `pcntl_fork()`: the child inherits the armed and fired probes and the
  collection state. From then on each process records and reports only its
  own hits; neither sees the other's.
- ZTS: the user opcode handler table is process-wide, so only the first
  thread that runs a request owns the probes. When a second PHP thread starts
  a request (FrankenPHP workers, ext-parallel), collection goes off with
  `a second PHP thread started in this process (FrankenPHP, ext-parallel); opcov collects on one PHP thread only`.
- A panic inside opcov turns collection off for the process with
  `panic in <entry point>: <message>`, puts the original handler back on
  every probed opcode and lets PHP continue.
