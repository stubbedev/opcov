# Compatibility

What opcov does alongside other extensions, threads, processes and each PHP
language feature, and which test proves it. OPcache and the JIT have their own
matrix in [support.md](support.md#opcache-and-jit).

## Other extensions

opcov registers a user opcode handler for every opcode. It saves the handlers
other extensions registered before it and forwards to them whenever a probe is
not armed or keeps the opcode running, so their handlers keep working; without a
previous handler it returns `ZEND_USER_OPCODE_DISPATCH`.

| Extension | Status | Proven by |
|---|---|---|
| Xdebug with `coverage` in `xdebug.mode` or `XDEBUG_MODE` | refused (`xdebug-coverage`): both measure coverage | `tests/phpt/compat/coverage-tools.phpt` |
| Xdebug in `develop`, `debug`, `profile`, `trace` or `gcstats` modes | supported; Xdebug's own user opcode handlers (`develop` registers `BEGIN_SILENCE`/`END_SILENCE` for `xdebug.scream`) are chained | `tests/phpt/compat/xdebug-develop.phpt`, `coverage-tools.phpt` |
| PCOV with `pcov.enabled=1` | refused (`pcov-enabled`) | `tests/phpt/compat/coverage-tools.phpt` |
| PCOV loaded with `pcov.enabled=0` | supported (PCOV installs nothing) | `tests/phpt/compat/coverage-tools.phpt` |
| phpdbg (replaces `zend_execute_ex` and calls every handler through `zend_vm_call_opcode_handler()`) | supported; a probe that already fired comes back through the user opcode handler and is a no-op | `tests/phpt/engine/execute-ex.phpt` |
| uopz | not tested: nixpkgs has no uopz build for PHP 8.4 to 8.6 | |
| Blackfire, Tideways | not tested: both are proprietary binaries that the test environment does not build | |
| SPX | not tested yet (available in nixpkgs for 8.4 and 8.5); it replaces `zend_execute_ex` only while profiling, which phpdbg's test covers in kind | |

The compatibility tests run when `OPCOV_XDEBUG_EXTENSION` and
`OPCOV_PCOV_EXTENSION` name the Xdebug and PCOV builds for the PHP under test,
and skip otherwise.

## Threads

`zend_set_user_opcode_handler()` is process-wide. The first PHP thread that runs
a request owns the probes. When a second PHP thread starts (FrankenPHP workers,
ext-parallel runtimes), collection stops for the process with
`second-thread`, and the second thread never touches opcov's state.

| Setup | Status | Proven by |
|---|---|---|
| ZTS CLI, one PHP thread | supported | the whole `.phpt` suite on the ZTS builds |
| A second PHP thread (ext-parallel) | refused (`second-thread`) | not tested: nixpkgs marks `ext-parallel` broken for 8.4 to 8.6 |
| FrankenPHP or Octane worker mode | out of scope for 1.x | |

## Processes

| Situation | Behaviour | Proven by |
|---|---|---|
| `pcntl_fork()` while collecting | parent and child keep collecting and report independently | `tests/phpt/fork/fork.phpt` |
| `exit()` inside measured code while collecting | shutdown functions run; `stop($test)` attributes the hits up to the exit, `write_report()` writes a complete report | `tests/phpt/process/exit.phpt` |
| A fatal error (for example memory exhaustion) inside measured code | the same as `exit()` | `tests/phpt/process/fatal.phpt` |
| A fatal error inside opcov's own engine calls | caught at the call, resumed after opcov's frames return; opcov stays usable in shutdown functions | `tests/phpt/engine/bailout.phpt` |
| A Rust panic inside opcov | collection stops with `panic`, every probed opcode gets its handler back, PHP keeps running | `tests/phpt/engine/panic.phpt`, `probe-panic.phpt` |

The extension decides nothing about writing reports: the PHPUnit package writes
the report, and after an `exit()` or fatal error that means from a shutdown
function.

## Language features

| Feature | Status | Proven by |
|---|---|---|
| Fibers (`Fiber::suspend()` inside a measured branch) | supported | `tests/phpt/outcomes/contexts.phpt` |
| Generators, including a `yield` between a branch and its successor | supported; the outcome belongs to the collection where the branch ran | `tests/phpt/language/features.phpt`, `outcomes/contexts.phpt`, `loops/loops.phpt` |
| `__destruct` running while an exception unwinds | supported | `tests/phpt/language/features.phpt` |
| `goto` | supported | `tests/phpt/loops/loops.phpt` |
| `declare(ticks)` | supported | `tests/phpt/language/features.phpt` |
| `eval()` | measured as part of the file that calls it | `tests/phpt/filter/eval.phpt` |
| `include` inside a function | supported; each include is measured | `tests/phpt/language/features.phpt`, `lines/reinclude.phpt` |
| `static fn`, closures, first-class callables (`strlen(...)`) | supported | `tests/phpt/language/features.phpt`, `lines/closures.phpt` |
| Property hooks (8.4) | supported | `tests/phpt/lines/hooks.phpt`, `language/features.phpt` |
| Lazy objects (8.4), initialised on first access | supported | `tests/phpt/language/features.phpt` |
| Pipe operator (8.5) | supported | `tests/phpt/language/pipe.phpt` |
| Partial function application (8.6) | supported; the closure compiled at run time is adopted on its first probe hit | `tests/phpt/units/partial-application.phpt` |
| Traits | supported; a trait method shared by classes shares its probes | `tests/phpt/lines/traits.phpt` |
