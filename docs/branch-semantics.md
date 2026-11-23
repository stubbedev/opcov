# Branch semantics

opcov reports three kinds of coverage for every measured function: executable
lines, branch **outcomes** and **loop findings**. This page defines what each
outcome kind and each loop finding means, which of them count toward a
verdict, and where opcov differs from Xdebug and PCOV. The report fields are
defined in [report-schema.md](report-schema.md); the mechanism that records
them is in [how-it-works.md](how-it-works.md).

## Where outcomes come from

opcov does not parse source to find branches. It takes the control-flow graph
PHP itself builds for the compiled code (`zend_build_cfg`) and names every
edge by the opcode that ends the block it leaves (`EdgeKind`,
`crates/opcov-core/src/kind.rs`; the mapping is `opcode_edge_kind` in
`crates/opcov-core/src/cfg/classify.rs`). Consequences:

- An outcome exists only where PHP compiled a branching opcode.
- Every outcome has a **site**, the index of the opcode it comes from. All
  outcomes of one site are the outcomes of one source construct, so several
  constructs on one line (`$a ?? 1, $b ?? 2`) stay apart.
- An outcome's `line` is the line of its opcode. Spans (columns) are added
  from the source when a report is read (`opcov-syntax`; see
  [cli.md](cli.md#opcov-doctor)).
- Measured files are compiled without `switch` jump tables, so every `case`
  is a comparison and a jump ([configuration.md](configuration.md#jump-tables)).

## Outcome kinds

Each kind below lists the opcode it comes from and an example from the
`.phpt` suite (`tests/phpt/outcomes/fixtures/constructs.php`, checked by
`tests/phpt/outcomes/constructs.phpt`).

### `IfTrue`, `IfFalse`

`JMPZ`/`JMPNZ`: the condition of an `if`, an `elseif`, or a full ternary
`$a ? $b : $c` was true or false. An `if` without `else` still has `IfFalse`,
so an implicit `else` is an outcome of its own.

```php
function cond($a)
{
    if ($a) {
        return 'then';
    }
    return 'implicit else';
}
```

`cond(1)` takes `IfTrue`, `cond(0)` takes `IfFalse`. An `if`/`elseif`/`else`
chain has one `IfTrue`/`IfFalse` pair per condition; `chain(0)` in the fixture
takes `IfFalse` on both the `if` and the `elseif` line.

The condition of a `for`, `while` or `do`-`while` is not an `If*` outcome but a
loop test (`LoopBack`/`LoopExit`, below).

### `ShortCircuit`, `Evaluate`

`JMPZ_EX`/`JMPNZ_EX`: `&&`, `||`, `and`, `or`. `ShortCircuit` means the left
operand decided the result; `Evaluate` means the right operand ran.

```php
function logic($a, $b)
{
    return ($a && $b) || $a;
}
```

`logic(1, 1)`: the `&&` evaluates `$b`, then the `||` short-circuits.
`logic(0, 1)`: the `&&` short-circuits, then the `||` evaluates.

### `CoalesceValue`, `CoalesceFallback`

`COALESCE`: `??` and `??=` found a non-null value, or used the fallback.

```php
function coalesce($a)
{
    return $a ?? 'fallback';
}
```

### `TernaryValue`, `TernaryFallback`

`JMP_SET`: the short ternary `?:` kept its truthy left operand, or used the
fallback.

```php
function ternary($a)
{
    return $a ?: 'fallback';
}
```

### `NullsafeNull`, `NullsafeValue`

`JMP_NULL`: `?->` met `null` and short-circuited, or went on.

```php
function nullsafe($o)
{
    return $o?->p;
}
```

### `MatchArm(n)`, `MatchNoArm`

A `match` took arm `n`, or no arm matched and it threw `UnhandledMatchError`
(the `MATCH_ERROR` block). Arms are numbered from 0 in source order; an arm
with several conditions is one arm, and `default` is an arm like any other.
Both compilations PHP uses are covered: a `MATCH` jump table for constant arms,
and a chain of `===` comparisons (`match (true)`, non-constant arms).

```php
function table($a)
{
    return match ($a) {
        1, 2 => 'low',
        3 => 'mid',
        default => 'high',
    };
}
```

`table(2)` takes `MatchArm(0)`, `table(3)` `MatchArm(1)`, `table(9)` and
`table('2')` `MatchArm(2)`. A `match` without `default` has `MatchNoArm`
instead of a default arm.

### `SwitchCase(n)`, `SwitchDefault`

A `switch` jumped to case `n` (counted from 0 in source order), or to
`default`, or past the `switch` when it has no `default`.

```php
function cases($a)
{
    switch ($a) {
        case 1:
            return 'one';
        case 2:
            return 'two';
        default:
            return 'many';
    }
}
```

Each case's outcome sits on the line of its `case` comparison; `SwitchDefault`
sits on the line of the last `case`, where the jump to `default` is compiled.
The edges between the comparisons ("try the next case") are `Flow`, not
outcomes.

### `LoopEnter`, `LoopSkip`

`FE_RESET_R`/`FE_RESET_RW`: a `foreach` started iterating, or skipped the loop
entirely. An array never skips, even when empty: `FE_RESET` on an array always
falls through to the first fetch, which then exits. An empty `Iterator`,
`IteratorAggregate` or generator, an empty plain object and a value that is
not iterable take `LoopSkip`.

These two outcomes are reported but **do not count toward verdicts**. Whether
a `foreach` ran zero times is the loop's `zero` finding (below), which is the
same for an empty array and an empty iterator.

### `LoopBack`, `LoopExit`

The test of a loop entered the body, or left the loop: the `FE_FETCH_*` of a
`foreach`, and the condition of a `for`, `while` or `do`-`while`.

### `Catch`

`CATCH`: a `catch` clause caught the exception. The outcome sits on the line
of its `catch`. An exception that no clause of the `try` matches records no
`Catch` outcome.

```php
function caught(callable $f)
{
    try {
        $f();
        return 'none';
    } catch (LogicException $e) {
        return 'logic';
    } catch (RuntimeException $e) {
        return 'runtime';
    }
}
```

### `Finally`, `FinallyException`

A `finally` block has two entries. `Finally` (`FAST_CALL`) is normal flow: the
`try` finished, or `return`, `break` or `continue` left it. `FinallyException`
is the exception unwinding into the block; it is not a CFG edge, so opcov reads
the block's fast-call variable when the block starts (see
[how-it-works.md](how-it-works.md#catch-and-finally)).

```php
function finished(callable $f)
{
    try {
        $f();
    } finally {
        echo "finally\n";
    }
}
```

### `Engine`

A jump the engine compiles for its own reasons, not for a branch in the
source. Reported, **excluded from verdicts**:

| Opcode | Source that produces it |
|---|---|
| `JMP_FRAMELESS` | an unqualified call of a frameless internal function inside a namespace (`str_contains($s, 'a')` in `namespace App`): PHP checks at run time whether `App\str_contains` exists |
| `BIND_INIT_STATIC_OR_JMP` | a `static` variable with a non-constant initializer (`static $n = f();`): the initializer runs on the first call only |
| `ASSERT_CHECK` | `assert()`: jumps over the assertion when assertions are off at run time (`zend.assertions=0`) |
| `SWITCH_LONG`/`SWITCH_STRING` fallthrough | a jump table's fallthrough; measured files compile without jump tables, so this does not occur in them |

### `Jump`

An edge opcov could not classify: a chain of comparisons that is neither a
`match` nor a `switch`, or an opcode with an unexpected number of successors.
`Jump` counts toward verdicts, and `opcov doctor` lists it as an outcome
without a source construct. It indicates a gap in opcov's classification
and is worth a bug report with `\opcov\get_debug_info()` of the file.

### `Flow` (never reported)

`Flow` names the edges that are not decisions: falling into the next block,
an unconditional `JMP`, the edge from a `CATCH` to the next `catch` clause, the
"next case" edge of a `switch` or `match` comparison chain. `Flow` edges are
part of the CFG (`\opcov\get_debug_info()`) but are never outcomes.

## What counts toward a verdict

`EdgeKind::counts_toward_verdict` decides which outcomes a consumer must see
taken:

| Kind | Reported | Counts |
|---|---|---|
| `Flow` | no | no |
| `Engine` | yes | no |
| `LoopEnter`, `LoopSkip` | yes | no |
| every other kind | yes | yes |

`opcov gate` reports an unseen outcome only when it counts; `opcov convert`
writes only counting outcomes as branches; `opcov doctor` fails only on
unmatched outcomes that count ([cli.md](cli.md)).

## Loop findings

Every structured loop has two findings, in addition to the outcomes of its
test:

- **`zero`**: the tests in which the loop ran zero times.
- **`second`**: the tests in which one activation of the loop ran its body two
  or more times.

A loop exercised only with one element reports both as missing; one exercised
with `[]`, `[a]` and `[a, b]` reports neither. A loop that only ever runs once
hides the bugs that depend on the iteration count.

The report's loop `kind` is the compiled shape:

| Kind | Source | Zero | Two or more |
|---|---|---|---|
| `Foreach` | `foreach` | the first fetch found nothing, or `FE_RESET` skipped | the body is entered after a fetch that was not the activation's first |
| `PreTest` | `for`, `while` | the test left an activation that had not entered the body | the test entered the body in an activation that already entered it |
| `PostTest` | `do`-`while` | impossible: `zero` is `null` | the test jumped back into the body |

- `zero` is also `null` for a pre-test loop whose condition is a constant true
  (`while (true)`, `for (;;)`), which can only be left from inside the body.
- An **activation** is one execution of the loop from its entry. A recursive
  call, another call of the function or a later entry into the loop starts a
  new one; a generator's activation spans its resumptions.
- `foreach` counts iterations from the iterator's position. It never calls an
  `Iterator` method on opcov's behalf: iterating a user `Iterator` under opcov
  calls `rewind`, `valid`, `current` and `next` exactly as without it.
- Loops built from `goto` have no shape and are not reported.

From `tests/phpt/loops/fixtures/loops.php`:

```php
function each_value(iterable $items)
{
    $count = 0;
    foreach ($items as $item) {
        $count++;
    }
    return $count;
}

function at_least_once(int $n)
{
    do {
        $n--;
    } while ($n > 0);
}
```

`each_value([])` reports `zero` (through `LoopEnter` and `LoopExit`);
`each_value(new ArrayIterator([]))` reports `zero` (through `LoopSkip`);
`each_value([1, 2])` reports `second`. `at_least_once(2)` reports `second`;
its `zero` is `null`.

## Executable lines

The rules are in [configuration.md](configuration.md#executable-lines). Two
differences from PCOV and Xdebug follow from them.

### Multi-line expressions

A line that only consumes the value of a multi-line expression gets no probe,
so it is covered only when one of its own operands ran. From
`tests/phpt/lines/fixtures/multiline.php`:

```php
function either(bool $a, bool $b): bool
{
    return $a
        || $b;
}

function pick(int $n): string
{
    return match ($n) {
        1 => 'one',
        2 => 'two',
        default => 'many',
    };
}
```

`either(true, false)` leaves line 5 uncovered: `$b` never ran.
`pick(1)` leaves the `default` line uncovered, although the `match` result is
consumed there. PCOV and Xdebug mark such a line covered whenever the
expression ran; the parity allowlist (`tests/diff/allowlist.json`) lists these
cases.

### Files that only declare

A file whose top-level code only declares classes, functions and constants has
no executable line in opcov. PCOV and Xdebug report its final `return` line,
attributed to whichever test loaded the file first, which makes per-test data
depend on test order and on how a run is split across processes. The decision
and its consequences are in
[ADR 0003](adr/0003-declarations-only-files.md).

## Differences from Xdebug

The parity suite ([parity.md](parity.md)) checks opcov against Xdebug's branch
coverage on everything both can express; these are the differences by design:

- **`??`, `?:` and `?->` are split.** Xdebug does not report the two sides of
  these operators as separate branches; opcov reports both outcomes of each.
- **Outcomes, not paths.** Xdebug's path coverage enumerates the paths through
  a function, whose number grows exponentially with its branches. opcov
  reports every outcome (edge) and never enumerates paths; the PHPUnit driver
  refuses path coverage instead of inventing paths
  ([phpunit.md](phpunit.md#phpunit-133-the-opcov-driver)).
- **Loops run zero and two or more times.** Xdebug cannot express either;
  opcov reports both per loop.
- **Outcomes have kinds.** Xdebug reports opcode-level branches with target
  lines; opcov names each outcome (`CoalesceFallback`, `MatchArm(2)`, ...) and
  maps it to the source construct with an exact span.
- **`switch` without jump tables.** A `switch` that Xdebug sees as one
  jump-table decision is a chain of case decisions in opcov, because measured
  files are compiled without jump tables.
- **Lines.** Xdebug counts the run-time `DECLARE_CLASS` line of a class that
  extends or implements and misses some multi-line nullsafe operands; opcov
  follows PCOV's executable-line rules.

## Differences from PCOV

- PCOV reports lines only; it has no outcomes and no loop findings.
- PCOV does not see property hooks; opcov measures them like methods.
- The multi-line expression and declarations-only cases above.
