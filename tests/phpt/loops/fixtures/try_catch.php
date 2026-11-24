<?php
final class Failure extends Exception
{
}

function attempt(int $i, int $failures): int
{
    if ($i < $failures) {
        throw new Failure();
    }
    return $i;
}

function retry_for(int $tries, int $failures): ?int
{
    for ($i = 0; $i < $tries; $i++) {
        try {
            return attempt($i, $failures);
        } catch (Failure $e) {
            echo '';
        }
    }
    return null;
}

function retry_forever(int $failures): int
{
    $i = 0;
    while (true) {
        try {
            return attempt($i, $failures);
        } catch (Failure $e) {
            $i++;
        }
    }
}

function guarded_while(int $n, int $failures): void
{
    while ($n > 0) {
        try {
            attempt($n, $failures);
        } catch (Failure $e) {
            echo '';
        }
        $n--;
    }
}

function guarded_foreach(array $items, int $failures): void
{
    foreach ($items as $item) {
        try {
            attempt($item, $failures);
        } catch (Failure $e) {
            echo '';
        } finally {
            echo '';
        }
    }
}

function retry_nested(int $tries, int $failures, array $notes): ?int
{
    for ($i = 0; $i < $tries; $i++) {
        try {
            return attempt($i, $failures);
        } catch (Failure $e) {
            foreach ($notes as $note) {
                echo '';
            }
        }
    }
    return null;
}
