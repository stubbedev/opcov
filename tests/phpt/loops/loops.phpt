--TEST--
Loops report zero and two-or-more iterations separately: foreach over arrays (by value and by reference), ArrayIterator, a user Iterator (whose methods opcov never calls itself) and a generator; for, while, do-while (never zero), while (true) (never zero), break and continue with levels, goto out of a loop, recursion, a generator yielding inside a loop and a loop inside finally
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
$file = __DIR__ . '/fixtures/loops.php';
require $file;
$iterables = [
    'array' => static fn (array $values) => $values,
    'ArrayIterator' => static fn (array $values) => new ArrayIterator($values),
    'Iterator' => static fn (array $values) => new Numbers($values),
    'generator' => static fn (array $values) => generated($values),
];
foreach ($iterables as $name => $make) {
    foreach ([[], [1], [1, 2]] as $values) {
        echo $name, ' ', json_encode($values), "\n", opcov_branches($file, static fn () => each_value($make($values)));
    }
}
Numbers::$calls = [];
each_value(new Numbers([1, 2]));
echo 'Iterator calls: ', implode(',', Numbers::$calls), "\n";
foreach ([[], [1], [1, 2]] as $values) {
    echo 'by reference ', json_encode($values), "\n", opcov_branches($file, static fn () => each_reference($values));
}
echo "one element only\n", opcov_branches($file, static fn () => each_value([1]));
echo "[], [a] and [a, b] in one test\n", opcov_branches($file, static function (): void {
    each_value([]);
    each_value([1]);
    each_value([1, 2]);
});
foreach ([0, 1, 2] as $n) {
    echo "for $n\n", opcov_branches($file, static fn () => counted($n));
    echo "while $n\n", opcov_branches($file, static fn () => waiting($n));
    echo "do-while $n\n", opcov_branches($file, static fn () => at_least_once($n));
    echo "while (true) $n\n", opcov_branches($file, static fn () => forever($n));
}
echo "continue 2 and break 2\n", opcov_branches($file, static fn () => nested([['next', 'x'], ['a', 'stop'], ['never']]));
echo "recursion: the outer activation runs twice, each inner one zero times\n", opcov_branches($file, static fn () => recursive(1, 2));
echo "generator yielding inside the loop, resumed by the caller\n", opcov_branches($file, static function (): void {
    foreach (yielding(2) as $_) {
    }
});
echo "loop inside finally\n", opcov_branches($file, static fn () => in_finally(2));
echo "goto out after two iterations, then a fresh activation that runs zero times\n", opcov_branches($file, static function (): void {
    leave_by_goto(5, 2);
    leave_by_goto(0, 9);
});
$model = \opcov\executable([$file])[$file]['functions'];
echo 'do-while zero: ', json_encode($model['at_least_once']['loops'][0]['zero']), "\n";
echo 'while (true) zero: ', json_encode($model['forever']['loops'][0]['zero']), "\n";
?>
--EXPECT--
array []
  each_value: LoopEnter@52 LoopExit@52 Foreach@52:zero
array [1]
  each_value: LoopEnter@52 LoopBack@52 LoopExit@52
array [1,2]
  each_value: LoopEnter@52 LoopBack@52 LoopExit@52 Foreach@52:second
ArrayIterator []
  each_value: LoopSkip@52 Foreach@52:zero
ArrayIterator [1]
  each_value: LoopEnter@52 LoopBack@52 LoopExit@52
ArrayIterator [1,2]
  each_value: LoopEnter@52 LoopBack@52 LoopExit@52 Foreach@52:second
Iterator []
  each_value: LoopSkip@52 Foreach@52:zero
Iterator [1]
  each_value: LoopEnter@52 LoopBack@52 LoopExit@52
Iterator [1,2]
  each_value: LoopEnter@52 LoopBack@52 LoopExit@52 Foreach@52:second
generator []
  each_value: LoopSkip@52 Foreach@52:zero
  generated: LoopEnter@44 LoopExit@44 Foreach@44:zero
generator [1]
  each_value: LoopEnter@52 LoopBack@52 LoopExit@52
  generated: LoopEnter@44 LoopBack@44 LoopExit@44
generator [1,2]
  each_value: LoopEnter@52 LoopBack@52 LoopExit@52 Foreach@52:second
  generated: LoopEnter@44 LoopBack@44 LoopExit@44 Foreach@44:second
Iterator calls: rewind,valid,current,next,valid,current,next,valid
by reference []
  each_reference: LoopEnter@60 LoopExit@60 Foreach@60:zero
by reference [1]
  each_reference: LoopEnter@60 LoopBack@60 LoopExit@60
by reference [1,2]
  each_reference: LoopEnter@60 LoopBack@60 LoopExit@60 Foreach@60:second
one element only
  each_value: LoopEnter@52 LoopBack@52 LoopExit@52
[], [a] and [a, b] in one test
  each_value: LoopEnter@52 LoopBack@52 LoopExit@52 Foreach@52:zero Foreach@52:second
for 0
  counted: LoopExit@68 PreTest@68:zero
while 0
  waiting: LoopExit@75 PreTest@75:zero
do-while 0
  at_least_once: LoopExit@84
while (true) 0
  forever: IfTrue@90 LoopBack@89
for 1
  counted: LoopBack@68 LoopExit@68
while 1
  waiting: LoopBack@75 LoopExit@75
do-while 1
  at_least_once: LoopExit@84
while (true) 1
  forever: IfTrue@90 IfFalse@90 LoopBack@89 PreTest@89:second
for 2
  counted: LoopBack@68 LoopExit@68 PreTest@68:second
while 2
  waiting: LoopBack@75 LoopExit@75 PreTest@75:second
do-while 2
  at_least_once: LoopBack@84 LoopExit@84 PostTest@83:second
while (true) 2
  forever: IfTrue@90 IfFalse@90 LoopBack@89 PreTest@89:second
continue 2 and break 2
  nested: LoopEnter@98 LoopBack@98 LoopEnter@99 LoopBack@99 IfTrue@100 IfFalse@100 IfTrue@103 IfFalse@103 Foreach@98:second Foreach@99:second
recursion: the outer activation runs twice, each inner one zero times
  recursive: IfTrue@113 LoopBack@112 LoopExit@112 PreTest@112:zero PreTest@112:second
generator yielding inside the loop, resumed by the caller
  yielding: LoopBack@121 LoopExit@121 PreTest@121:second
loop inside finally
  in_finally: Finally@129 LoopBack@131 LoopExit@131 PreTest@131:second
goto out after two iterations, then a fresh activation that runs zero times
  leave_by_goto: IfTrue@140 IfFalse@140 LoopBack@139 LoopExit@139 PreTest@139:zero PreTest@139:second
do-while zero: null
while (true) zero: null
