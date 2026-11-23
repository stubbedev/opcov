# Troubleshooting

Every time opcov refuses to collect, `opcov\CannotCollectException` carries a
message that ends with a link to one entry on this page. `opcov\get_status()`
names the same entry in `off_code`, lists every conflict it finds under
`conflicts`, and `php --ri opcov` prints them too. `opcov doctor` runs all of
these checks in a child PHP process that uses your INI files and prints each
problem with its fix.

The heading of each entry is the stable code that the message links to.

## panic

**Message:** `panic in <entry point>: <message>`

opcov hit a bug and caught it (a Rust panic) at the named engine entry point.
Collection is off for the rest of the process. opcov has restored the original
handler of every probed opcode, so PHP keeps running at full speed. The report
for this process cannot be trusted and is refused.

**Fix:** rerun the suite. If the panic comes back, file a bug with the message,
the output of `opcov doctor` and the output of `opcov\get_debug_info()` for the
file that was running.

## second-thread

**Message:** `a second PHP thread started in this process (FrankenPHP, ext-parallel); opcov collects on one PHP thread only`

Probes change opcode handlers that every thread of the process shares, so
opcov collects on the first PHP thread only. Once a second thread starts
(FrankenPHP workers, `ext-parallel` runtimes), collection stops for the whole
process.

**Fix:** collect coverage in a single-threaded CLI process. A ZTS build is fine
as long as only one PHP thread runs. Coverage under FrankenPHP or Octane worker
mode is out of scope for opcov 1.x.

## ini-registration-failed

**Message:** `the engine refused to register opcov's INI settings`

PHP refused opcov's `opcov.*` INI entries when the extension started. That
happens when another loaded extension already registered settings with the
same names.

**Fix:** load opcov only once (`php --ini` lists every INI file PHP reads) and
check that no other extension declares `opcov.*` settings.

## not-enabled-at-startup

**Message:** `opcov.enabled was 0 when PHP started; set opcov.enabled=1 in php.ini or with -d`

opcov installs nothing unless `opcov.enabled=1` is set when PHP starts. The
setting is `PHP_INI_SYSTEM`/`PHP_INI_PERDIR`, so `ini_set()` cannot turn it on
later.

**Fix:** set `opcov.enabled=1` in an INI file or with `php -d opcov.enabled=1`.
The `opcov-phpunit` launcher does this for you.

## no-reserved-slot

**Message:** `PHP has no free op_array.reserved slot left for opcov (other extensions use all of them)`

opcov tracks the code it measures in one of the engine's `op_array.reserved`
slots. Zend extensions loaded before opcov took all of them.

**Fix:** load fewer Zend extensions (each one that tracks op arrays takes a
slot), or load opcov before them.

## not-enabled

**Message:** `opcov.enabled is 0; set opcov.enabled=1`

opcov started, but `opcov.enabled` is 0 for this request (a per-directory INI
file can switch it off).

**Fix:** set `opcov.enabled=1` for the request that collects coverage.

## compile-hooks-missing

**Message:** `zend_compile_file or zend_compile_string is not set`

opcov wraps the engine's compile hooks to see every file and `eval()` as it is
compiled. Another extension cleared one of them.

**Fix:** find the extension that replaces the compile hooks without chaining to
the previous ones (`php -m` lists them) and load PHP without it while
collecting coverage.

## invalid-selection

**Message:** the reason `opcov.collect_paths` or `opcov.exclude_paths` cannot be used, for example a path that does not exist, a list file that cannot be read, or a value that is not UTF-8.

**Fix:** correct the setting named in the message. See
[configuration.md](configuration.md) for the syntax of both settings.

## collect-paths-empty

**Message:** `opcov.collect_paths is not set; set it to the files and directories to measure`

opcov measures only the files that `opcov.collect_paths` selects, and nothing is
selected.

**Fix:** set `opcov.collect_paths` to the source directories, or let the
`opcov-phpunit` launcher derive it from the `<source>` element of
`phpunit.xml`.

## invalid-exclude-pattern

**Message:** `opcov.exclude_pattern is not a valid PCRE pattern with delimiters: <pattern>`

**Fix:** write the pattern as `preg_match()` expects it, delimiters included,
for example `opcov.exclude_pattern=~/generated/~`.

## exclude-pattern-failed

**Message:** `opcov.exclude_pattern failed to match against <path>`

PCRE returned an error (for example a backtracking limit) while matching the
pattern against a compiled file's path. opcov cannot decide whether that file is
excluded.

**Fix:** simplify the pattern, or raise `pcre.backtrack_limit`.

## snapshot-failed

**Message:** `cannot analyse code compiled from <file>: <reason>`

opcov could not build the control-flow model of a function in a measured file.
The file's coverage would be incomplete, so the report is refused.

**Fix:** file a bug with the message and the file (or a reduced copy). Until it
is fixed, exclude the file with `opcov.exclude_paths`.

## nothing-selected

**Message:** `opcov.collect_paths selected none of the <n> files compiled during this run`

At least one test ran, yet no compiled file is inside `opcov.collect_paths`.
The settings most likely point at the wrong directory (a relative path resolved
against an unexpected working directory, a typo, or a symlink to elsewhere).

**Fix:** compare `opcov\get_status()['skipped_files']`, which names every
compiled file with the reason it was skipped, with `opcov.collect_paths`.

## opcache-shared-sapi

**Message:** `OPcache is enabled under the <sapi> SAPI, which shares the compiled code between processes, ...; set opcache.enable=0, or collect coverage under the CLI SAPI`

Probes rewrite opcode handlers in place. Under the CLI, OPcache's "shared"
memory belongs to the one process and is writable. Under FPM, CGI and other
server SAPIs it is shared between all workers, so a probe in one worker would
rewrite the code every other worker runs.

**Fix:** run the tests under the CLI SAPI, or set `opcache.enable=0` for the
SAPI that collects coverage.

## opcache-protect-memory

**Message:** `opcache.protect_memory=1 makes the compiled code read-only, so probes cannot be installed; set opcache.protect_memory=0`

**Fix:** set `opcache.protect_memory=0` (the default) while collecting coverage.

## opcache-file-cache

**Message:** `opcache.file_cache=<dir> stores the compiled code, probes included, on disk for other processes; set opcache.file_cache= (empty) and opcache.file_cache_only=0`

The file cache would persist opcode handlers that point at opcov's probes into
files that later processes load, with or without opcov.

**Fix:** leave `opcache.file_cache` empty while collecting coverage.

## opcache-jit

**Message:** `opcache.jit=<mode> with opcache.jit_buffer_size=<size> turns the JIT on, and JIT-compiled code never reads the opcode handlers that probes replace; set opcache.jit=disable`

JIT-compiled code jumps straight between machine-code blocks and never looks at
`zend_op.handler`, so probes would not fire. The tracing JIT also writes its own
counters into the same handler slots. opcov refuses every JIT mode (`tracing`,
`function`, `on` and the numeric `CRTO` forms) whenever
`opcache.jit_buffer_size` is above zero. `docs/adr/0004-opcache-jit.md`
explains why a per-function exclusion is not possible.

**Fix:** set `opcache.jit=disable` (the default since PHP 8.4), or
`opcache.jit_buffer_size=0`. `opcache.jit=off` also works, but it lets a script
turn the JIT on at run time with `ini_set()`, and opcov then refuses the report.

## opcache-preload

**Message:** `opcache.preload=<script> compiles code before opcov installs its hooks, so preloaded code gets no probes; set opcache.preload= (empty)`

OPcache runs the preload script while PHP starts, before the first request,
which is when opcov installs its compile hooks. Preloaded classes and functions
would have no probes and show up as never executed.

**Fix:** leave `opcache.preload` empty while collecting coverage.

## opcache-optimizer

**Message:** `opcache.optimization_level=<level> lets the OPcache optimizer remove and merge code, so lines and outcomes disappear from the report; set opcache.optimization_level=0`

The optimizer removes dead and redundant code, folds constants and merges
blocks after the compiler is done and before opcov sees the code. A statement
that ran but was optimized away has no opcode left to probe, so it shows up as
not executable, and branches decided at compile time lose their outcomes.

**Fix:** set `opcache.optimization_level=0`. OPcache keeps caching compiled code
in the process, so `opcache.enable_cli=1` still saves the recompilation of
files that are included more than once.

## xdebug-coverage

**Message:** `Xdebug is loaded with mode "<mode>" (from xdebug.mode or XDEBUG_MODE), which includes coverage: ...; set xdebug.mode to modes without coverage (for example xdebug.mode=off), or do not load Xdebug`

Xdebug's coverage mode hooks the same opcodes that opcov probes, and both tools
then measure each other. Xdebug's other modes (`develop`, `debug`, `profile`,
`trace`, `gcstats`) work alongside opcov; see
[compatibility.md](compatibility.md).

**Fix:** remove `coverage` from `xdebug.mode` or from the `XDEBUG_MODE`
environment variable, which takes precedence over the INI setting, or do not
load Xdebug.

## pcov-enabled

**Message:** `PCOV is loaded with pcov.enabled=1: ...; set pcov.enabled=0, or do not load PCOV`

PCOV replaces the engine's instruction loop when it is enabled and records
every executed line itself, which runs every probe again and measures the
probes as well.

**Fix:** set `pcov.enabled=0` (PCOV then installs nothing) or do not load
PCOV.

# Problems `opcov doctor` finds outside the extension

`opcov doctor` reports these when the extension cannot report anything itself,
because PHP did not load it or the self-test could not run.

### extension-not-loaded

The PHP binary `opcov doctor` ran (`--php`, default `php` from `PATH`) does not
load opcov.

**Fix:** add `extension=opcov` (or the full path of `opcov.so`) to an INI file
that PHP reads. `php --ini` lists the loaded `php.ini` and the scanned
directory; `opcov doctor` prints both too.

### build-mismatch

PHP refused to load `opcov.so` because it was built for another PHP: another
minor version (`Module compiled with module API=…`) or another thread-safety
mode (`Module compiled with build ID=API…,NTS` against `…,TS`, or the dynamic
loader's `undefined symbol: executor_globals` for an NTS build in a ZTS PHP and
`undefined symbol: tsrm_get_ls_cache` the other way round). PHP prints the
raw numbers while it starts; `opcov doctor` translates them into the PHP
version and build to install. `php --ri opcov` cannot show this, since the
extension is not loaded; once it is loaded, its `Built for` row names the PHP
version, thread safety and module API it was built for.

**Fix:** install the opcov build that matches the PHP version and the ZTS/NTS
build of the PHP that runs the tests.

### version-mismatch

The extension and the `opcov` CLI come from different compatibility lines (a
different major version, or before 1.0 a different minor version).

**Fix:** install both from the same opcov release.

### self-test-failed

opcov is loaded and reports no problem, yet the probe self-test (a two-line
branch in a temporary file) did not produce the expected lines and outcome.

**Fix:** file a bug with the full `opcov doctor` output.
