# ADR 0004: OPcache and JIT support matrix and refusals

## Status

Accepted. Decides #11.

## Context

Probes write `zend_op.handler` of the measured code in place (ADR 0002). With
OPcache, the opcodes of cached files live in OPcache's memory, and OPcache can
transform them before opcov sees them. The constraints turbocov's `conflicts.c`
lists were checked against php-src 8.5 (`ext/opcache/ZendAccelerator.c`,
`zend_persist.c`, `jit/zend_jit.c`) and reproduced with `.phpt` tests.

How the two hook chains are ordered matters for everything below. OPcache
replaces `zend_compile_file` in `accel_post_startup()`, after every extension's
`MINIT`. opcov wraps the compile hooks at the first request (`RINIT`), so it sees
what OPcache returns: code that is already optimized, persisted into OPcache's
memory and, with the JIT on, compiled to machine code.

## Decision

opcov checks the settings at request startup and again on every
`ensure_can_collect()`, `start()`, `stop()`, `write_report()` and `get_status()`.
A conflict found at request startup keeps opcov from installing anything: no
user opcode handlers, no compile hooks, no probes. `ensure_can_collect()` then
throws `CannotCollectException` with a message that names the setting, its
value, the value to use and the troubleshooting entry. `get_status()['conflicts']`,
`php --ri opcov` and `opcov doctor` list every conflict found, not only the first.

| Setup | Verdict | Code |
|---|---|---|
| OPcache not loaded, or `opcache.enable=0`, or CLI with `opcache.enable_cli=0` | supported | |
| CLI (or phpdbg) with `opcache.enable_cli=1`, `opcache.optimization_level=0`, JIT off, no file cache, no preload | supported | |
| OPcache enabled under any other SAPI (FPM, CGI, Apache, ...) | refused | `opcache-shared-sapi` |
| `opcache.optimization_level` other than 0 | refused | `opcache-optimizer` |
| JIT on: `opcache.jit` = `tracing`/`on`/`1`/`yes`/`true`, `function` or a valid `CRTO` number, with `opcache.jit_buffer_size` > 0 | refused | `opcache-jit` |
| `opcache.file_cache` set (with or without `opcache.file_cache_only`) | refused | `opcache-file-cache` |
| `opcache.preload` set | refused | `opcache-preload` |
| `opcache.protect_memory=1` | refused | `opcache-protect-memory` |

The JIT check mirrors `zend_jit_config()`: `disable`, `off`, `0`, `no`, `false`
and the empty string are off; an invalid value disables the JIT in OPcache and is
off for opcov too; a number is on only when `zend_jit_parse_config_num()` would
accept it. `opcache.jit` is `PHP_INI_ALL`: with `off` (not `disable`) and a JIT
buffer a script can turn the JIT on with `ini_set()`. opcov then refuses from
that point on (`tests/phpt/opcache/runtime-jit.phpt`); `disable` is the value
the fix recommends.

### Why each setup is refused

- **Shared SAPIs.** In the CLI, OPcache's "shared" memory is an anonymous mapping
  private to the process, so writing handlers into it affects only that process.
  Under FPM or CGI the mapping is shared by every worker: one worker's probe
  would rewrite the code all of them run, and its fire-once restore would race
  with the others.
- **Optimizer.** `zend_optimize_script()` runs inside OPcache's compile, before
  opcov's hook returns, and removes or merges code (dead code elimination, SCCP,
  constant folding, jump threading). Statements that ran but were optimized away
  have no opcode to probe and show up as not executable; branches decided at
  compile time lose their outcomes.
- **JIT.** JIT code jumps between machine-code blocks and never reads
  `opline->handler`, so probes would not fire. The tracing JIT also stores its
  own counter handlers in the same slots, which probes would overwrite.
- **File cache.** OPcache would write handlers pointing into opcov's probe
  into files that later processes load.
- **Preload.** The preload script is compiled during startup, before opcov's
  hooks exist, so preloaded code gets no probes and reads as never executed.
- **protect_memory.** OPcache maps its memory read-only between its own writes;
  installing a probe would crash.

### Investigation: probing before the optimizer (negative)

The idea was to install probes on the unoptimized op arrays and let them survive
optimization. That would need opcov's compile hook *inside* OPcache's: hooking
`zend_compile_file` at `MINIT` makes opcov `accelerator_orig_compile_file`, which
OPcache calls on a cache miss. It does not work:

- `zend_optimize_script()` calls `zend_revert_pass_two()`, rewrites the opcode
  array (removes, moves and merges opcodes, renumbers jump targets) and calls
  `zend_redo_pass_two()`, which sets every handler again through
  `zend_vm_set_opcode_handler()`. Installed probes are overwritten, and the
  opcode indexes they were planned for no longer exist.
- `zend_persist_op_array_ex()` then copies the op arrays into OPcache's memory.
  Units are keyed by the `opcodes` address, which changes.
- Even with probes carried through, code the optimizer removed has no opcode
  left to fire, so its lines and outcomes could only be reported as executable
  and never covered, which is wrong for code that ran.

A working alternative that is *not* implemented: OPcache's blacklist
(`opcache.blacklist_filename`, prefix-matched) makes `opcache_compile_file()`
call the original compiler directly, so measured files would bypass the
optimizer and the cache while every other file keeps both. opcov cannot verify a
blacklist from inside the engine (OPcache does not export the lookup), so it
would have to trust the configuration or detect, per file, whether the op array
came from OPcache. This is a possible follow-up issue; the speed gain is small in
the CLI, where OPcache's cache lives for one process.

### Investigation: per-function JIT exclusion versus global refusal (negative)

- With `opcache.jit=function` (trigger "on script load"), the JIT compiles every
  function inside OPcache's compile, during persistence, before opcov's hook
  returns. There is nothing left to exclude by the time opcov sees the code.
- The hot-counter triggers install counter handlers in `zend_op.handler` during
  persistence too, and `ZEND_FUNC_INFO`/`zend_jit_op_array` are internal to
  OPcache (not exported), so opcov cannot mark a measured op array as "do not
  JIT".
- `zend_jit_check_support()` disables the JIT when any user opcode handler is
  registered at startup. opcov registers its handlers at the first request, which
  is too late. Registering them at `MINIT` would turn the JIT off for the whole
  process, but with a warning printed at startup (which breaks tools that parse
  their output) and with every file compiled during startup getting the user
  opcode handler. A clear refusal with `opcache.jit=disable` as the fix is
  simpler and predictable.

So the JIT is refused globally.

### Running the PHPUnit package with OPcache

The `opcov-phpunit` launcher writes `opcov.ini` into a per-run directory that it
appends to `PHP_INI_SCAN_DIR` before it re-executes PHP, so every PHP process of
the run (paratest workers included, which inherit the environment) reads it. The
OPcache settings fit into the same mechanism without the package passing or
validating `-d` flags for children:

- users who want OPcache's cache in the CLI pass the supported values the same
  way as any other setting, `opcov-phpunit -d opcache.enable_cli=1 -d
  opcache.optimization_level=0 -d opcache.jit=disable`; the launcher writes
  `-d` settings into the same INI file, so workers inherit them;
- when a conflicting value is set anyway (in `php.ini`, a scanned file or the
  environment), the extension refuses in every process: the package's existing
  `ensure_can_collect()` call at startup aborts with the message, the setting and
  the value to use. No second validation in PHP is needed, and none can drift
  from the extension's.

The package itself is unchanged by this ADR.

## Consequences

- Coverage runs with OPcache in the CLI keep the compile cache but give up the
  optimizer and the JIT; a measured run is slower than an unmeasured JIT run.
- Probes are installed into OPcache's process-private memory. A file included
  again in the same process is served from the cache with the same `opcodes`
  address and shares its unit (`tests/phpt/opcache/supported.phpt`).
- Every refusal is reproduced by `tests/phpt/opcache/refused.phpt`,
  `shared-sapi.phpt` (php-cgi) and `runtime-jit.phpt`; on PHP 8.4, where OPcache
  is a separate `opcache.so`, these tests need `OPCOV_OPCACHE_EXTENSION`.
