<?php

declare(strict_types=1);

$test = static function (): void {
    $ledger = new Ledger();
    $ledger->open('a', 100);
    $ledger->open('b');
    $ledger->transfer('a', 'b', 30);
    if ($ledger->balance('b') !== 30 || $ledger->statement('a') !== [-30] || !$ledger->audit()) {
        throw new LogicException('ledger arithmetic is broken');
    }
};

return array_fill(0, 20_000, $test);
