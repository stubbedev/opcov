# ADR 0002: Branch-outcome and loop probes

## Status

Accepted. Implements #6 (branch outcomes), #7 (loops) and #28 (outcome sites).

## Context

Line probes (#5) fire once and restore the real handler. A branching opcode has
several outcomes, so its probe (a `SOURCE`) must stay armed until every outcome
was seen since the last `reset()`, and it must learn which outcome each run
took. #6 named two ways to learn it:

- **Successor watch** (generic): when the source fires, put one-shot `WATCH`
  probes on the first opcode of each successor block and remember
  `(frame, source)` as pending; the next probe that fires in that frame is on
  the successor that was taken, because a jump transfers control directly.
- **Evaluate in the probe**: decide the outcome from the source opcode's
  operand before the real handler runs, for the opcodes whose decision is a
  pure function of a value already computed.

## Spike

Micro-benchmark: a tight loop of `??` and an `if` over a comparison,
10 000 000 iterations, best of 7 rounds, PHP 8.5.12 NTS (HYBRID VM), opcov
built with `--release --features engine-selftest` (the feature adds
`opcov\selftest_force_watch()`, which makes every source use the watch design):

```php
for ($i = 0; $i < $n; $i++) {
    $x = $values[$i & 1] ?? 1;
    if ($x > 1) { $sum++; } else { $sum--; }
}
```

| Run | ns per iteration | vs. no extension |
|---|---|---|
| extension not loaded | 13.82 - 14.25 | - |
| loaded, file not selected | 13.84 - 14.10 | within noise |
| measured, every outcome seen (`$values = [null, 5]`) | 13.95 - 14.11 | within noise (-1.0 % to +1.3 %) |
| measured, one outcome of each source never seen, evaluate design | 88.37 - 89.82 | about 37 ns per armed source run |
| same, watch design | 180.30 - 180.36 | about 83 ns per armed source run |

The first three rows answer #6's acceptance: once every outcome of a source was
seen, the source and its successors carry their real handlers again, so a loop
running it is as fast as without opcov. The last two rows are the price of a
source that stays armed (a test that never takes one of its outcomes): the
watch design costs two probe runs and four handler writes per run, the
evaluate design one probe run. A Laravel feature test was not measured in this
spike (no Laravel suite in the repository); the benchmark harness (#14)
covers real suites.

## Decision

Evaluate where the decision is a pure function of an operand, fall back to the
watch everywhere else and whenever an evaluation cannot decide without running
user code.

| Source opcode | Decision |
|---|---|
| `JMPZ`, `JMPNZ`, `JMPZ_EX`, `JMPNZ_EX`, `JMP_SET` | truthiness of op1, as `i_zend_is_true`; an undefined variable (the handler warns, and an error handler may throw) and an object whose class has a cast handler other than `zend_std_cast_object_tostring` (GMP, SimpleXML, FFI) fall back to the watch, so no user code and no warning ever runs on opcov's behalf |
| `COALESCE` | op1 set and not `null` after dereferencing (an undefined variable is `null` here, as `BP_VAR_IS` reads it) |
| `JMP_NULL` | op1 `null` or undefined; an undefined variable that the handler warns about falls back to the watch |
| `MATCH` | the jump-table lookup of op1 (`long` or `string` key, else the default target) |
| `JMP` (the default jump of a `match`/`switch` chain), `FAST_CALL` | always the jump target |
| `FE_RESET_*`, `FE_FETCH_*` | watch: deciding would call `getIterator()`, `valid()` or resume a generator |
| `CATCH`, `SWITCH_*`, `JMP_FRAMELESS`, `BIND_INIT_STATIC_OR_JMP`, `ASSERT_CHECK` | watch |

### Smart branches

`pass_two()` fuses a comparison (`IS_IDENTICAL`, `IS_SMALLER`, `ISSET_ISEMPTY_*`,
`INSTANCEOF`, `TYPE_CHECK`, ...) with the `JMPZ`/`JMPNZ` after it
(`IS_SMART_BRANCH_JMPZ/JMPNZ` in the comparison's `result_type`): the comparison
jumps itself and the `JMPZ` never runs, so a probe on it would never fire, and
the comparison has not run yet when a probe on it fires. While such a source is
armed, opcov clears the smart-branch flag of the comparison and gives it the
handler the VM picks for the plain result type (computed once per unit with the
user handlers unregistered); the comparison then writes its boolean and the
`JMPZ` runs and is evaluated. When the source saturates, the flag and the fused
handler come back, so the steady state is the engine's own code.

### Per-thread state and unwinding

The pending watches and the loop activations live in the engine state, which
only the owner thread touches (the TSRM approach of #2 and #19: one collecting
thread per process). A pending watch is keyed by the frame (`execute_data`):
probes in nested frames (an iterator's `valid()`, a resumed generator) leave
it alone, and any other armed probe that fires in the same frame discards it,
because control did not reach a successor. opcov chains a
`zend_throw_exception_hook` that discards every pending watch when an exception
is thrown, so an exception thrown inside the condition, inside an iterator
method or by an error handler never completes an outcome; a re-thrown exception
(`CATCH` without a match) needs no hook, because the watched successors of a
`CATCH` are reachable only through it. `reset()`, request shutdown and the
destruction of a unit's op array drop the pending watches and activations that
remain.

### Loops (#7)

- `foreach`: the probe on `FE_FETCH_*` reads, before the handler runs, whether
  this activation already fetched: `Z_FE_POS_P(op1) > 0` for arrays read by
  value, the position of `EG(ht_iterators)[Z_FE_ITER_P(op1)]` for arrays by
  reference and plain objects, `zend_object_iterator.index != (zend_ulong) -1`
  for `Iterator`, `IteratorAggregate` and generators. The body entered after a
  fetch that was not the first is "two or more"; the exit taken by the first
  fetch, or `FE_RESET`'s skip, is "zero" (an array never skips: `FE_RESET_R` on
  an array always falls through to the first fetch). Nothing calls user code.
- `for`/`while` (`PreTest`): the jump that enters the loop from outside gets a
  `LOOP_ENTRY` probe that starts an activation keyed by
  `(execute_data, unit, loop)`; the test entering the body when the activation
  already entered it is "two or more", the test leaving a fresh activation is
  "zero". The map holds entries only for loops whose probes are armed and is
  emptied when the loop saturates, at `reset()` and when the unit dies; a frame
  that left the loop by `break`, `return`, `goto` or an exception leaves a stale
  entry that the next entry probe of that frame address overwrites.
- `do-while` (`PostTest`): the back edge is "two or more"; zero iterations are
  impossible and the report says so with `zero: null`, as for a pre-test loop
  whose condition is a constant true.
- A loop's probes stay armed until zero (when possible), two or more, and every
  outcome of its test were seen since the last reset.

### Catch and finally

A `catch` clause's outcome is the `Catch` edge of its `CATCH` opcode (the class
matched). A `finally` block has two entries: `FAST_CALL` (normal flow, always
the jump target) and the exception unwinding into it, which is not a CFG edge.
The first opcode of every `finally` block carries a `FINALLY_ENTRY` probe that
reads the block's fast-call variable (op1 of the `FAST_RET` at the try/catch
element's `finally_end`): the engine stores the exception there before jumping
in, `FAST_CALL` stores `NULL`. A non-null value records `FinallyException`.

### Outcome sites and ids (#28)

`opcov_core::cfg::Plan` derives the outcomes and loops of a function from its
CFG, in one place for the extension, the report and `opcov-syntax`: every
non-`Flow` edge, plus one `FinallyException` per reachable `finally` block,
sorted by (site, successor block, kind) and numbered `o0`, `o1`, ...; loops are
numbered `l0`, ... in header order. The site is the index of the source opcode
in the function. The report records it, so consumers group outcomes by site
instead of assuming "the n-th outcome of a kind on a line".

## Consequences

- Steady-state cost after saturation is zero; an armed source costs about one
  probe run (evaluated) or two (watched) per execution.
- `LoopEnter`/`LoopSkip` stay in the report but no longer count toward verdicts:
  arrays never take `LoopSkip`, and the loop's `zero` list carries the verdict.
- Recording never calls user code and never emits a warning; when it cannot
  decide that way it watches, and when even the watch cannot tell (two outcomes
  into one block from a watched source) it records nothing rather than guess.
