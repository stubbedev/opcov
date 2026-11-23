<?php

declare(strict_types=1);

interface Shape
{
    public function area(): float;
}

final class Square implements Shape
{
    public const SIDES = 4;

    public function __construct(private float $side)
    {
    }

    public function area(): float
    {
        return $this->side ** 2;
    }
}
