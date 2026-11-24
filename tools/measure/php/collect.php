<?php

declare(strict_types=1);

use Opcov\Measure\Config;
use Opcov\Measure\Run;
use Opcov\Measure\Workload;

require __DIR__ . '/autoload.php';

if ($argc !== 3) {
    fwrite(STDERR, "usage: collect.php <sources directory> <runner file>\n");
    exit(2);
}

$workload = Workload::open($argv[1], $argv[2]);
$tests = $workload->tests();
$run = new Run(Config::fromEnvironment());
$run->beginTest('load');
$variables = $workload->load();
$run->endTest();
foreach ($tests as $index => $test) {
    $run->beginTest("test#$index");
    $test($variables);
    $run->endTest();
}
$run->finish();
