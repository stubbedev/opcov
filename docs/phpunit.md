# PHPUnit and Pest

The Composer package `opcov/phpunit` connects the opcov extension to PHPUnit 12,
PHPUnit 13, Pest 4 and Pest 5. It provides:

| Piece | What it does |
|---|---|
| `OpCov\PHPUnit\Extension` | A PHPUnit extension that measures every test and writes the canonical opcov report ([report-schema.md](report-schema.md)) when the run ends. Works with PHPUnit 12, PHPUnit 13, Pest 4, Pest 5 and paratest. |
| `vendor/bin/opcov-phpunit` | A launcher that turns PHPUnit's `<source>` into the opcov INI settings and starts the test runner with them. |
| `OpCov\PHPUnit\Driver` | A php-code-coverage driver for PHPUnit 13.3 and newer, so PHPUnit's own coverage reports (`--coverage-html`, `--coverage-clover`, ...) can use opcov instead of Xdebug or PCOV. |
| `vendor/bin/opcov-cov` | Converts an opcov report into php-code-coverage's serialized `.cov` format, for `phpcov merge` and PHPUnit's report tooling. |
| `vendor/bin/opcov` | The `opcov` command ([cli.md](cli.md)), a native binary the package's Composer plugin downloads and verifies ([install.md](install.md#the-opcov-command)). |

## Install

```sh
composer require --dev opcov/phpunit
```

The package lives in `php/phpunit` of this repository and is published to
Packagist from its read-only split,
[stubbedev/opcov-phpunit](https://github.com/stubbedev/opcov-phpunit), which a
workflow updates on every push to `main` and every release tag; send issues and
pull requests here.

The extension itself is installed separately ([install.md](install.md)) and
loaded in `php.ini` (`extension=opcov`); loading it with `opcov.enabled=0`, the
default, changes nothing. The package's major version follows the extension's:
at startup it refuses an extension from another compatibility line (a different
major version, or before 1.0.0 a different minor version) with
`the opcov extension X is not compatible with opcov/phpunit Y`. `ext-pcntl` and
`ext-posix` are needed by the launcher only.

The package is an optional Composer plugin that installs the `opcov` command
as `vendor/bin/opcov`. Allow it with
`composer config allow-plugins.opcov/phpunit true`; without that, Composer
skips it and `vendor/bin/opcov` downloads the binary on first use
([install.md](install.md#the-opcov-command)).

## Register the extension

```xml
<phpunit bootstrap="vendor/autoload.php">
    <source>
        <include>
            <directory>src</directory>
        </include>
    </source>
    <extensions>
        <bootstrap class="OpCov\PHPUnit\Extension">
            <parameter name="output" value="build/opcov.json"/>
        </bootstrap>
    </extensions>
</phpunit>
```

Pest reads the same `phpunit.xml`, so the same block works for Pest 4 and Pest 5.

| Parameter | Required | Meaning |
|---|---|---|
| `output` | yes | The report file. Relative paths are relative to the directory of the PHPUnit configuration file. Missing directories are created. May contain `{TEST_TOKEN}` (see [Parallel runs](#parallel-runs-with-paratest)). |
| `root` | no | The directory that the report's file keys are relative to (`paths.root`), relative to the configuration file's directory; it must exist. Default: the working directory. |
| `absolute_paths` | no | `true` writes absolute file keys (`paths.mode` `absolute`); `false` (default) writes relative ones. Cannot be combined with `root`. |

The extension fails the run, with exit code 3 and a message on standard error
starting with `opcov:`, when the extension is not loaded or not compatible, a
parameter is invalid, `\opcov\ensure_can_collect()` refuses collection (for
example `opcov.enabled` is `0` or no file was selected), or the report cannot
be written. It never writes a report for a run that did not collect. Exit code 3
is distinct from PHPUnit's own codes (1 failures, 2 errors). To run the tests
without coverage while the extension is registered, pass `--no-extensions`.

## Turn collection on

The opcov INI settings are `PHP_INI_SYSTEM | PHP_INI_PERDIR`
([configuration.md](configuration.md)): they must be set before PHP starts, so
the PHPUnit extension cannot set them. Choose one of two ways.

### With the launcher

```sh
vendor/bin/opcov-phpunit vendor/bin/phpunit
vendor/bin/opcov-phpunit vendor/bin/pest --parallel
vendor/bin/opcov-phpunit -d memory_limit=1G vendor/bin/paratest -p 8
```

Usage: `opcov-phpunit [-d <setting>=<value>]... <runner script> [runner arguments]...`

The launcher:

1. finds the PHPUnit configuration the way PHPUnit does: the file or directory
   given with `-c`/`--configuration` among the runner arguments, otherwise
   `phpunit.xml`, `phpunit.dist.xml` or `phpunit.xml.dist` in the working
   directory (`--no-configuration` is refused, since it leaves nothing to
   measure);
2. reads `<source><include>` and `<source><exclude>` (`<directory>` and `<file>`,
   relative to the configuration file, `*` patterns expanded) and resolves every
   entry with `realpath()`. An entry that does not exist, or a `<directory>` with
   a `prefix` or a `suffix` other than `.php`, is refused with the element in the
   message: opcov selects whole directories and cannot express such filters;
3. writes `collect_paths.txt` and `exclude_paths.txt` listfiles and an
   `opcov.ini` with `opcov.enabled=1`, `opcov.collect_paths=@collect_paths.txt`,
   `opcov.exclude_paths=@exclude_paths.txt` and the `-d` settings given to the
   launcher, into the private directory
   `<sys_get_temp_dir()>/opcov-<uid>/<sha256 of the configuration path and settings>/`
   (mode 0700; a directory that someone else owns or others can access is
   refused). Files are written to a temporary name and renamed, so concurrent
   runs of the same configuration are safe;
4. replaces itself (`pcntl_exec`, no shell) with `PHP_BINARY <runner script>
   <runner arguments>` and `PHP_INI_SCAN_DIR` extended by that directory (an
   inherited `PHP_INI_SCAN_DIR` is kept; when there is none, PHP's compiled-in
   scan directory is kept through the leading empty entry).

The settings travel through `PHP_INI_SCAN_DIR` instead of `-d` because the
environment reaches every PHP process the runner starts: paratest workers,
process-isolated tests and PHPT tests. paratest starts its workers with
`PHP_BINARY` and only the options given with `--passthru-php`, so `-d` flags of
the parent never reach them. For the same reason, PHP options given to the
`php` binary that runs the launcher are not inherited; pass them to the
launcher with `-d` instead. `opcov.*` settings are refused there, since the
launcher derives them from the configuration.

### Setting the INI yourself

Set the settings in `php.ini`, a file in the INI scan directory, or with `-d`:

```sh
php -d opcov.enabled=1 -d opcov.collect_paths=src -d opcov.exclude_paths=src/Generated vendor/bin/phpunit
```

`-d` does not reach paratest workers; use an INI file or `--passthru-php` there.

## What a test is attributed

The extension subscribes to PHPUnit's events (verified in PHPUnit 12.5 and
13.4, the same in both):

| Event | Action |
|---|---|
| `Test\PreparationStarted` | `\opcov\start()`, before `setUp()` and the `#[Before]` methods, so fixture code counts for the test as it does in PHPUnit's own coverage. |
| `Test\Prepared` | Notes that the test body will run. |
| `Test\Skipped`, `Test\MarkedIncomplete`, `Test\ConsideredRisky` | The test's hits will be discarded. |
| `Test\Finished` | `\opcov\stop($testId)` (or `\opcov\stop()` for a discarded test), then `\opcov\reset()`. |
| `Test\PreparationErrored`, `Test\PreparationFailed`, `Test\Failed`, `Test\Errored` before `Test\Prepared` | Ends the collection at once and attributes it, because PHPUnit emits no `Test\Finished` for a test that was never prepared. |
| `Application\Finished` | `\opcov\ensure_can_collect()`, `\opcov\write_report()`, then a check that the file starts with `{"schema":"opcov-report/1"`. |

Every started collection is stopped exactly once: a test that starts while
another is still collected, an event of a different test during a collection,
or a run that ends during a collection fails the run.

So, as in php-code-coverage:

- passing, failing and erroring tests are attributed;
- skipped, incomplete and risky tests are not; their executable lines stay in
  the report;
- a test that errors in `setUp()` is attributed what ran up to the error; what
  its `tearDown()` runs afterwards is not attributed. (php-code-coverage counts
  such a test as risky when it performed no assertion and drops it; that
  decision is internal to PHPUnit's runner and has no event.)

Test ids are `$event->test()->id()`: `Class::method`, plus `#<data set name>`
for data providers (`#0` for unnamed sets). Pest's ids are the generated class
and method, for example
`P\Tests\ShippingTest::__pest_evaluable_cost_depends_on_weight#dataset "heavy"`.

Code that runs while no test is collected (bootstrap, data providers,
`setUpBeforeClass()`) is attributed to no test; opcov re-arms those probes at
the next `start()`, so a test that runs the same lines still sees them.

Process-isolated tests (`#[RunInSeparateProcess]`, `processIsolation="true"`)
and PHPT tests run in a child process that does not bootstrap PHPUnit
extensions, so they are not attributed. A run in which every test is isolated
(`--process-isolation`) compiles no measured file in the PHPUnit process and
fails with `opcov.collect_paths selected none of the N files compiled during
this run`.

## Parallel runs with paratest

Each worker must write its own report. Put `{TEST_TOKEN}` into `output`; it is
replaced by the worker's `TEST_TOKEN` environment variable, which paratest sets:

```xml
<parameter name="output" value="build/opcov.{TEST_TOKEN}.json"/>
```

```sh
vendor/bin/opcov-phpunit vendor/bin/paratest -c phpunit-paratest.xml -p 8
opcov merge build/opcov.*.json -o build/opcov.json
```

`{TEST_TOKEN}` without `TEST_TOKEN` set, and `TEST_TOKEN` set without
`{TEST_TOKEN}` in `output` (every worker would overwrite the same file), fail
the run, so a serial and a parallel run need separate configurations. The
paratest parent process bootstraps the extension too but runs no test and
writes no report. `opcov merge` is described in [cli.md](cli.md).

## PHPUnit 13.3+: the opcov driver

PHPUnit 13.3 added custom code coverage drivers
([#6586](https://github.com/sebastianbergmann/phpunit/pull/6586)): the
`driver` attribute of `<coverage>` names a class that extends
php-code-coverage's `SebastianBergmann\CodeCoverage\Driver\Driver`
(php-code-coverage 14.3.5 or newer; PHPUnit's `Runner\CodeCoverage::instantiateDriver()`
checks the class, constructs it without arguments, or with the `Filter` when the
constructor requires one, and calls `setGranularity()`):

```xml
<coverage driver="OpCov\PHPUnit\Driver"/>
```

```sh
vendor/bin/opcov-phpunit vendor/bin/phpunit --coverage-html build/html
```

PHPUnit then uses opcov for every coverage report it writes, and decides
attribution itself. Do not register `OpCov\PHPUnit\Extension` in the same
configuration: both would call `\opcov\start()` for the same test, and the
second call fails. PHPUnit 12 and Pest 4 (which runs on PHPUnit 12) have no
driver setting; use the extension and `opcov-cov` there. PHPT tests always use
php-code-coverage's built-in driver selection (`Runner\Phpt\CodeCoverageBootstrapper`)
and cannot use opcov.

The driver collects lines and, on request, branches: `start()` calls
`\opcov\start()`; `stop()` calls `\opcov\stop()` and `\opcov\reset()` and
returns `RawCodeCoverageData::fromLineAndBranchCoverage()` with `1` for executed
lines and `-1` for the other executable lines of each file the first time the
driver reports it (`\opcov\executable()`); later tests only report their hits,
which keeps the cost per test proportional to the code it ran. Its name is
`opcov` and its version the extension's.

### Branch coverage

The driver returns `true` from `canCollectBranchCoverage()`, so
`--branch-coverage` (or `<coverage branchCoverage="true">`) makes PHPUnit's
reports show opcov's branch outcomes and loop findings ([branch-semantics.md](branch-semantics.md)):

```sh
vendor/bin/opcov-phpunit vendor/bin/phpunit --branch-coverage --coverage-html build/html
```

php-code-coverage's branch model is Xdebug's: a function's `branches` are the
blocks of its control-flow graph, and a report counts how many of them ran.
opcov records edges instead (which way each decision went), so each **outcome**
becomes one php-code-coverage branch, and so does each **loop finding**
([branch-semantics.md](branch-semantics.md#loop-findings-as-branches)); the
branch counts of a report are outcomes and loop findings taken out of those
reported:

| php-code-coverage field | Outcome `o<k>` | Loop `l<n>` |
|---|---|---|
| branch id | `k`; ids are stable per function and ordered by site | `N + 2n` (ran zero times) and `N + 2n + 1` (ran two or more times), where `N` is one more than the function's largest outcome number (0 when it has none) |
| `op_start`, `op_end` | the outcome's `site`, the index of the branching opcode; the outcomes of one construct share it | the loop's `site`, the opcode that tests the loop |
| `line_start`, `line_end` | the outcome's `line` | the loop's `line` |
| `hit` | `1` when the test took the outcome, else `0` | `1` when, in the test, the loop ran zero times (`N + 2n`) or one activation ran its body two or more times (`N + 2n + 1`), else `0` |
| `out`, `out_hit` | `[]`: an outcome is an edge, not a block with successors (the contract allows empty edges) | `[]` |

- Only outcomes that count toward a verdict become branches, as in
  `opcov convert`: `Engine` outcomes and the `LoopEnter`/`LoopSkip` outcomes of
  `foreach` are left out (an array never takes `LoopSkip`, so it would always
  show as missed); whether a `foreach` ran zero times is its loop's "ran zero
  times" branch. A loop whose `zero` is `null` (a `do-while`, `while (true)`,
  `for (;;)`) has only the "ran two or more times" branch. A `foreach` over
  `[]`, `[a]` and `[a, b]` in three tests takes all four of its branches:
  `LoopExit` in each, `LoopBack` with one and two elements, "ran zero times"
  with none and "ran two or more times" with two.
- With every branch a single line, the HTML report colours each line with
  branches by how many of them were taken and lists every branch with its
  tests in the file's branch view. It draws no decision markers and no
  control-flow graph, which need block successors (`out`).
- The first report of a file carries every function's branches; later tests
  carry only the functions they ran.

Function names follow php-code-coverage's convention, which is Xdebug's. Each
opcov function key ([report-schema.md](report-schema.md#function-keys)) maps
as follows; `<file>` becomes the absolute path:

| opcov key | php-code-coverage name | Counted in class and function totals |
|---|---|---|
| `{main}@<file>` | `{main}` | no (php-code-coverage has no unit for it) |
| `App\helper` | `App\helper` | yes |
| `App\Foo::bar` | `App\Foo->bar` | yes; a trait method is keyed by its trait, as php-code-coverage keys it |
| `App\Foo::$name::get` | `App\Foo->$name::get`, PHP's own name for the hook | no: php-code-coverage has no unit for property hooks |
| `{closure}@<file>:<a>-<b>` | `{closure:<file>:<a>-<b>}`, Xdebug's name | no: closures are no unit |
| `{closure}@<file>:<a>-<b>#<n>` | `{closure:<file>:<a>-<b>}#<n>` | no; Xdebug has no counterpart, since it names every closure of one line span the same |
| `class@anonymous@<file>:<a>-<b>[#<n>]::bar` | `class@anonymous@<file>:<a>-<b>[#<n>]->bar` | no: anonymous classes are no unit |

Branches of functions that are not counted still appear in the HTML branch
view and in the line colouring.

Function keys and report paths carry names and file names that are not valid
UTF-8 in opcov's canonical byte encoding
([report-schema.md](report-schema.md#bytes-that-are-not-utf-8)), for example
`Caf%E9::run` for a class named with the Latin-1 byte `\xE9`. The driver and
`opcov-cov` decode them back to the raw bytes, which is what PHP and
php-code-coverage use: the name above becomes `Caf\xE9->run`, and the file
paths php-code-coverage reads and reports are the real file names.

Path coverage stays refused: the driver keeps `canCollectPathCoverage()` at
`false`, so `--path-coverage` stops the run with php-code-coverage's
`opcov X does not support path coverage`. php-code-coverage 14 separates
`Granularity::LineAndBranch` from `Granularity::LineBranchAndPath`, and branch
coverage does not read `paths`; opcov never enumerates the paths through a
function, whose number grows exponentially with its branches, and does not
invent them.

## Converting to php-code-coverage's `.cov`

```sh
vendor/bin/opcov-cov build/opcov.json build/coverage.cov
```

The converter builds a `SebastianBergmann\CodeCoverage\CodeCoverage` with the
php-code-coverage version installed next to it and serializes it with that
version's own writer, so the file is what that PHPUnit would write with
`--coverage-php`:

| php-code-coverage | PHPUnit | Writer | Format |
|---|---|---|---|
| 12.5 | 12, Pest 4 | `Report\PHP::process()` | `<?php return \unserialize(...)` of the `CodeCoverage` object (its `__serialize()`: filter, settings, processed data, tests) |
| 14.x | 13, Pest 5 | `Serialization\Serializer::serialize()` | serialization format 4: `buildInformation`, `basePath`, `codeCoverage` (processed data), `testResults` |

How the data goes in:

- each report file is resolved against `paths.root` (or used as is in absolute
  mode) and its SHA-256 compared with the report's; a changed file stops the
  conversion, since php-code-coverage reads the sources;
- the report's executable lines are appended once as not executed; then, for
  each test, the lines it hit are appended under its id with status unknown,
  through `CodeCoverage::append()`, the path a driver's data takes in PHPUnit.
  php-code-coverage therefore applies its own processing as it would to PCOV's
  data: only lines its static analysis considers executable are kept, and
  `@codeCoverageIgnore` annotations are honoured;
- outcomes and loop findings become branches exactly as in the driver
  ([Branch coverage](#branch-coverage)), taken by the tests in an outcome's
  `hits` and a loop's `zero` and `second` lists, appended with the executable lines and
  then under each test that took them, so the `.cov` holds line and branch
  coverage as `--branch-coverage` would write it. With php-code-coverage 12.5,
  which has no branch-only mode, they go through
  `RawCodeCoverageData::fromXdebugWithPathCoverage()` with no paths. The
  driver name recorded in the format-4 build information is `opcov` with the version
  from the report's `generator.opcov`.

The output is written to a temporary file next to the target and renamed. Exit
code 0 on success, 2 on any error, with the input, output and cause on standard
error.
