--TEST--
Every outcome of every branching construct is reported alone, with the line it belongs to, including an implicit else, MATCH jump tables, match without an arm, catch, finally entered normally and by an exception, and several outcomes on one line
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
$file = __DIR__ . '/fixtures/constructs.php';
require $file;
$calls = [
    'cond(1)' => static fn () => cond(1),
    'cond(0)' => static fn () => cond(0),
    'chain(2)' => static fn () => chain(2),
    'chain(1)' => static fn () => chain(1),
    'chain(0)' => static fn () => chain(0),
    'logic(1, 1)' => static fn () => logic(1, 1),
    'logic(0, 1)' => static fn () => logic(0, 1),
    'words(1, 0)' => static fn () => words(1, 0),
    'words(0, 1)' => static fn () => words(0, 1),
    'coalesce(5)' => static fn () => coalesce(5),
    'coalesce(null)' => static fn () => coalesce(null),
    'coalesce_assign(set)' => static fn () => coalesce_assign(['k' => 1]),
    'coalesce_assign(unset)' => static fn () => coalesce_assign([]),
    'ternary(5)' => static fn () => ternary(5),
    'ternary(0)' => static fn () => ternary(0),
    'nullsafe(object)' => static fn () => nullsafe((object) ['p' => 1]),
    'nullsafe(null)' => static fn () => nullsafe(null),
    'table(2)' => static fn () => table(2),
    'table(3)' => static fn () => table(3),
    'table(9)' => static fn () => table(9),
    'table("2")' => static fn () => table('2'),
    'conditions(5)' => static fn () => conditions(5),
    'conditions(0)' => static fn () => conditions(0),
    'no_arm(2)' => static fn () => no_arm(2),
    'no_arm(7)' => static fn () => no_arm(7),
    'cases(1)' => static fn () => cases(1),
    'cases(2)' => static fn () => cases(2),
    'cases(3)' => static fn () => cases(3),
    'caught(none)' => static fn () => caught(static fn () => null),
    'caught(logic)' => static fn () => caught(static fn () => throw new LogicException()),
    'caught(runtime)' => static fn () => caught(static fn () => throw new RuntimeException()),
    'caught(error)' => static fn () => caught(static fn () => throw new Error()),
    'finished(normal)' => static fn () => finished(static fn () => null),
    'finished(exception)' => static fn () => finished(static fn () => throw new Error()),
    'one_line(1, null)' => static fn () => one_line(1, null),
    'one_line(null, 2)' => static fn () => one_line(null, 2),
];
foreach ($calls as $name => $call) {
    echo $name, "\n", opcov_branches($file, $call);
}
?>
--EXPECT--
cond(1)
  cond: IfTrue@4
cond(0)
  cond: IfFalse@4
chain(2)
  chain: IfTrue@12
chain(1)
  chain: IfFalse@12 IfTrue@14
chain(0)
  chain: IfFalse@12 IfFalse@14
logic(1, 1)
  logic: Evaluate@23 ShortCircuit@23
logic(0, 1)
  logic: ShortCircuit@23 Evaluate@23
words(1, 0)
  words: Evaluate@28 Evaluate@28
words(0, 1)
  words: ShortCircuit@28 Evaluate@28
coalesce(5)
  coalesce: CoalesceValue@33
coalesce(null)
  coalesce: CoalesceFallback@33
coalesce_assign(set)
  coalesce_assign: CoalesceValue@38
coalesce_assign(unset)
  coalesce_assign: CoalesceFallback@38
ternary(5)
  ternary: TernaryValue@44
ternary(0)
  ternary: TernaryFallback@44
nullsafe(object)
  nullsafe: NullsafeValue@49
nullsafe(null)
  nullsafe: NullsafeNull@49
table(2)
  table: MatchArm(0)@54
table(3)
  table: MatchArm(1)@54
table(9)
  table: MatchArm(2)@54
table("2")
  table: MatchArm(2)@54
conditions(5)
  conditions: MatchArm(0)@64
conditions(0)
  conditions: MatchArm(1)@64
no_arm(2)
  no_arm: MatchArm(1)@71
no_arm(7)
threw UnhandledMatchError
  no_arm: MatchNoArm@71
cases(1)
  cases: SwitchCase(0)@80
cases(2)
  cases: SwitchCase(1)@82
cases(3)
  cases: SwitchDefault@82
caught(none)
  (none)
caught(logic)
  caught: Catch@94
caught(runtime)
  caught: Catch@96
caught(error)
threw Error
  (none)
finished(normal)
finally
  finished: Finally@105
finished(exception)
finally
threw Error
  finished: FinallyException@106
one_line(1, null)
  one_line: CoalesceValue@112 CoalesceFallback@112 TernaryValue@112
one_line(null, 2)
  one_line: CoalesceFallback@112 CoalesceValue@112 TernaryFallback@112
