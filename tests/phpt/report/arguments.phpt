--TEST--
write_report() and stop() refuse invalid arguments with ValueError or TypeError, before changing any state
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures/app
--FILE--
<?php
require __DIR__ . '/fixtures/app/calc.php';
$target = sys_get_temp_dir() . '/opcov-never-written-' . getmypid() . '.json';
$calls = [
    'unknown key' => fn () => \opcov\write_report($target, ['roots' => '/']),
    'integer key' => fn () => \opcov\write_report($target, ['/']),
    'root type' => fn () => \opcov\write_report($target, ['root' => 1]),
    'empty root' => fn () => \opcov\write_report($target, ['root' => '']),
    'absolute type' => fn () => \opcov\write_report($target, ['absolute_paths' => 1]),
    'root with absolute' => fn () => \opcov\write_report($target, ['root' => '/', 'absolute_paths' => true]),
    'empty path' => fn () => \opcov\write_report(''),
    'nul in path' => fn () => \opcov\write_report("a\0b"),
    'missing root' => fn () => \opcov\write_report($target, ['root' => __DIR__ . '/absent']),
    'empty test' => function () { \opcov\start(); \opcov\stop(''); },
    'test type' => fn () => \opcov\stop([]),
    'collecting' => fn () => \opcov\write_report($target),
];
foreach ($calls as $name => $call) {
    try {
        $call();
        echo "$name: no error\n";
    } catch (Throwable $e) {
        echo "$name: ", get_class($e), ': ', str_replace(__DIR__, '{DIR}', $e->getMessage()), "\n";
    }
}
var_dump(\opcov\get_status()['collecting'], file_exists($target));
\opcov\stop();
$parameters = array_map(
    static fn (ReflectionParameter $p) => $p->getDeclaringFunction()->getName() . ': ' . $p->getType() . ' $' . $p->getName() . ($p->isOptional() ? ' = ' . json_encode($p->getDefaultValue()) : ''),
    [...(new ReflectionFunction('opcov\stop'))->getParameters(), ...(new ReflectionFunction('opcov\write_report'))->getParameters()],
);
echo implode("\n", $parameters), "\n";
--EXPECT--
unknown key: ValueError: opcov\write_report(): Argument #2 ($options) contains the unknown key "roots"; the known keys are "absolute_paths" and "root"
integer key: ValueError: opcov\write_report(): Argument #2 ($options) contains the unknown key 0; the known keys are "absolute_paths" and "root"
root type: TypeError: opcov\write_report(): Argument #2 ($options) key "root" must be of type string, int given
empty root: ValueError: opcov\write_report(): Argument #2 ($options) key "root" must be a non-empty path without NUL bytes
absolute type: TypeError: opcov\write_report(): Argument #2 ($options) key "absolute_paths" must be of type bool, int given
root with absolute: ValueError: opcov\write_report(): Argument #2 ($options) key "root" cannot be combined with "absolute_paths" => true
empty path: ValueError: opcov\write_report(): Argument #1 ($path) must not be empty
nul in path: ValueError: opcov\write_report(): Argument #1 ($path) must not contain any null bytes
missing root: RuntimeException: opcov\write_report(): cannot resolve the report root {DIR}/absent: No such file or directory (os error 2)
empty test: ValueError: opcov\stop(): Argument #1 ($test) must not be empty
test type: TypeError: opcov\stop(): Argument #1 ($test) must be of type ?string, array given
collecting: LogicException: opcov\write_report(): called while collecting; call opcov\stop() first
bool(true)
bool(false)
opcov\stop: ?string $test = null
opcov\write_report: string $path
opcov\write_report: array $options = []
