# ADR 0003: Files that only declare have no executable line

## Status

Accepted. Changes #5's rule "declarations-only files: record the file as
executed although the VM skips the opcode".

## Context

A file that only declares classes, interfaces, traits, enums, functions and
constants compiles to a main op array of declaration opcodes (`DECLARE_CLASS`,
`DECLARE_CLASS_DELAYED`, ... or nothing, when the declarations are bound at
compile time) followed by `RETURN 1`. With the stock executor,
`INCLUDE_OR_EVAL` skips a main op array that is a lone `RETURN` of a constant;
an extension that replaces `zend_execute_ex` (PCOV, Xdebug) runs it.

PCOV and Xdebug therefore report the `RETURN`'s line (usually the file's last
line) as executed, and opcov did the same by recording a hit for the skipped
opcode. That line runs once per process, during whichever test loads the file
first. Per-test attribution then depends on test order and on how a run is
split across processes: the PHPUnit package's parallel acceptance test (#10)
found a serial run attributing the line to `[0]` and a merged run of three
workers to `[0, 1]`, so merged parallel reports were not byte-identical to
serial ones.

## Decision

The main code of a file whose opcodes are all non-executable declaration
opcodes followed by a `RETURN` of a constant has no executable line and no
probe, whatever the executor. The rule is decided from the compiled opcodes
alone (`lines::declarations_only`), so it is the same with and without another
extension replacing `zend_execute_ex`. The file stays in the report with its
`sha256` and functions (its `{main}` with no lines, no outcomes, no loops).

Alternatives rejected:

- Keep the line and attribute it to no test with a distinct "covered by
  bootstrap" marker: a schema change for a line that runs no code of the file,
  and still wrong for the coverage percentage of a file only some tests load.
- Keep PCOV's behaviour: per-test data that depends on the order and split of
  the run, which breaks merging (#10) and the changed-code gate.

## Consequences

- One line fewer per class file than PCOV and Xdebug report; the differential
  suite (#13) has to document the difference.
- Top-level code that does run (a `require`, a `define()`, a `return [...]`
  configuration array) stays executable and is attributed to the test that ran
  it; that is real execution and remains order-dependent by nature.
