<?php

declare(strict_types=1);

function attempt(callable $operation): string
{
    try {
        $result = $operation();
        if ($result === null) {
            return 'empty';
        }
        return (string) $result;
    } catch (InvalidArgumentException | DomainException $e) {
        return 'invalid: ' . $e->getMessage();
    } catch (Throwable $e) {
        throw new RuntimeException('wrapped', 0, $e);
    } finally {
        echo 'done';
    }
}

function finallyReturns(bool $fail): string
{
    try {
        if ($fail) {
            throw new LogicException('fail');
        }
        return 'try';
    } finally {
        return 'finally';
    }
}

function nested(int $depth): int
{
    try {
        try {
            if ($depth > 1) {
                throw new UnderflowException('deep');
            }
            return $depth;
        } catch (UnderflowException) {
            return -1;
        }
    } catch (Exception) {
        return -2;
    }
}
