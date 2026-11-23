<?php

declare(strict_types=1);

use Opcov\Measure\Cli;

require __DIR__ . '/autoload.php';

exit(Cli::main(array_slice($argv, 1)));
