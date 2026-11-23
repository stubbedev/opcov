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
blocks and edges of the CFG.

## `eval()` and stream wrappers

- `eval()`'d code is measured when the file that called `eval()` is selected
  (nested `eval()`s are traced back to the outermost file). Its lines are
  reported under the engine's name for the code,
  `<file>(<line>) : eval()'d code`, as a file of its own.
- Files loaded through a stream wrapper (`phar://`, `vfs://`, a userland
  wrapper) are not selectable by path. They run unmeasured and are listed as
  skipped; nothing fails.
- A file name that cannot be resolved to an existing file (for example
  `Command line code` of `php -r`) or that is not valid UTF-8 is skipped.

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

A file that contains only declarations compiles to a single `RETURN` that the
VM skips when the file is included; opcov records that `RETURN`'s line as
executed anyway, as PCOV does.

`\opcov\executable($files)` compiles nothing: it reports the model of files
that were compiled in this process and omits files that were never loaded,
so a runner must treat a missing file as "not loaded" (php-code-coverage's
uncovered-file handling reads such files itself).

## Collection API

All functions live in the `opcov` namespace.

| Function | Behaviour |
|---|---|
| `ensure_can_collect(): void` | Throws `\opcov\CannotCollectException` (a `RuntimeException`) with the reason when collection is off. After at least one `start()`/`stop()` cycle it also throws when no compiled file was selected. |
| `start(): void` | Starts recording. Re-arms the probes that fired while collection was off (bootstrap code), so the collection sees them again. Throws `CannotCollectException` when collection is off and `LogicException` when already collecting. |
| `stop(): array` | Stops recording and returns `[file => ['lines' => [line => true]]]` for the probes that fired since `start()`, sorted by file and line. It does not re-arm them. Throws `LogicException` without a `start()`, and `CannotCollectException` when collection went off meanwhile. |
| `reset(): void` | Re-arms every probe that fired since the last `reset()`; its cost is proportional to the probes that fired, not to the probes installed. Throws `LogicException` while collecting. |
| `executable(array $files): array` | `[file => ['lines' => [line => []], 'functions' => [key => ['span' => null, 'outcomes' => [], 'loops' => []]]]]` for the given files that were compiled, in the shape of the report schema's `lines` and `functions`. |
| `get_status(): array` | `enabled`, `collecting`, `off_reason`, `selected_files`, `skipped_files` (file => reason) and `counters` (`units`, `probes_installed`, `probes_fired`, `probes_rearmed`, `jump_table_files`). |
| `get_debug_info(string $file): array` | `skip_reason`, `executable_lines`, `functions` (function key => CFG snapshot with blocks, edges, loops and diagnostics) and `probes` (function, opcode index, line, armed roles). |

A test runner calls `start()`, runs one test, calls `stop()` and then
`reset()`, so each test is measured on its own; without `reset()` a probe
that fired in one test stays fired in the next.

If the request ends while collecting, the pending hits are discarded at
request shutdown.

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
