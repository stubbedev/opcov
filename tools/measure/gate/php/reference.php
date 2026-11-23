<?php

declare(strict_types=1);

use Opcov\Measure\Gate\ChangedLines;
use Opcov\Measure\Gate\XdebugGate;
use Opcov\Measure\Json;

if ($argc !== 5) {
    fwrite(STDERR, "usage: reference.php <project root> <php-code-coverage .cov> <git diff> <output json>\n");
    exit(2);
}
[, $root, $cov, $diffFile, $output] = $argv;

require "$root/vendor/autoload.php";
require __DIR__ . '/autoload.php';

try {
    $diff = @file_get_contents($diffFile);
    if ($diff === false) {
        throw new \RuntimeException("cannot read $diffFile");
    }
    Json::write($output, XdebugGate::load($root, $cov)->evaluate(ChangedLines::fromGitDiff($diff, $diffFile)));
} catch (\Throwable $e) {
    $message = $e->getMessage();
    for ($previous = $e->getPrevious(); $previous !== null; $previous = $previous->getPrevious()) {
        $message .= ': ' . $previous->getMessage();
    }
    fwrite(STDERR, "reference gate: $message\n");
    exit(2);
}
