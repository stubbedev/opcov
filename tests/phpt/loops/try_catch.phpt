--TEST--
Loops whose body holds a try/catch are loops: a retry loop continued only by its catch handler (for and while (true)), a while and a foreach (with finally) whose catch rejoins the body, and a loop inside a catch handler, each reporting zero and two-or-more iterations
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
$file = __DIR__ . '/fixtures/try_catch.php';
require $file;
$runs = [
    'for retry, no tries' => static fn () => retry_for(0, 0),
    'for retry, first try succeeds' => static fn () => retry_for(3, 0),
    'for retry, second try succeeds' => static fn () => retry_for(3, 1),
    'for retry, every try fails' => static fn () => retry_for(2, 5),
    'while (true) retry, first try succeeds' => static fn () => retry_forever(0),
    'while (true) retry, third try succeeds' => static fn () => retry_forever(2),
    'while, zero iterations' => static fn () => guarded_while(0, 0),
    'while, two iterations, one caught' => static fn () => guarded_while(2, 2),
    'foreach, zero iterations' => static fn () => guarded_foreach([], 0),
    'foreach, two iterations, one caught' => static fn () => guarded_foreach([0, 1], 1),
    'nested, no tries' => static fn () => retry_nested(0, 0, [1]),
    'nested, two failures noted twice each' => static fn () => retry_nested(3, 2, [1, 2]),
];
foreach ($runs as $name => $run) {
    echo $name, "\n", opcov_branches($file, $run);
}
foreach (\opcov\executable([$file])[$file]['functions'] as $key => $function) {
    $loops = array_map(static fn (array $loop): string => $loop['kind'] . '@' . $loop['line'] . ' zero ' . json_encode($loop['zero']), $function['loops']);
    echo opcov_short_key($key), ': ', $loops === [] ? '(no loops)' : implode(', ', $loops), "\n";
}
?>
--EXPECT--
for retry, no tries
  retry_for: LoopExit@16 PreTest@16:zero
for retry, first try succeeds
  attempt: IfFalse@8
  retry_for: LoopBack@16
for retry, second try succeeds
  attempt: IfTrue@8 IfFalse@8
  retry_for: Catch@19 LoopBack@16 PreTest@16:second
for retry, every try fails
  attempt: IfTrue@8
  retry_for: Catch@19 LoopBack@16 LoopExit@16 PreTest@16:second
while (true) retry, first try succeeds
  attempt: IfFalse@8
  retry_forever: LoopBack@29
while (true) retry, third try succeeds
  attempt: IfTrue@8 IfFalse@8
  retry_forever: Catch@32 LoopBack@29 PreTest@29:second
while, zero iterations
  guarded_while: LoopExit@40 PreTest@40:zero
while, two iterations, one caught
  attempt: IfTrue@8 IfFalse@8
  guarded_while: Catch@43 LoopBack@40 LoopExit@40 PreTest@40:second
foreach, zero iterations
  guarded_foreach: LoopEnter@52 LoopExit@52 Foreach@52:zero
foreach, two iterations, one caught
  attempt: IfTrue@8 IfFalse@8
  guarded_foreach: LoopEnter@52 LoopBack@52 LoopExit@52 Catch@55 Finally@57 Foreach@52:second
nested, no tries
  retry_nested: LoopExit@65 PreTest@65:zero
nested, two failures noted twice each
  attempt: IfTrue@8 IfFalse@8
  retry_nested: Catch@68 LoopEnter@69 LoopBack@69 LoopExit@69 LoopBack@65 Foreach@69:second PreTest@65:second
{main}@try_catch.php: (no loops)
attempt: (no loops)
guarded_foreach: Foreach@52 zero []
guarded_while: PreTest@40 zero []
retry_for: PreTest@16 zero []
retry_forever: PreTest@29 zero null
retry_nested: Foreach@69 zero [], PreTest@65 zero []