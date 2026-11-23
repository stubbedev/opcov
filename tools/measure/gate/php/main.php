<?php

declare(strict_types=1);

use Opcov\Measure\Gate\GateCli;

require __DIR__ . '/autoload.php';

exit(GateCli::main(array_slice($argv, 1)));
