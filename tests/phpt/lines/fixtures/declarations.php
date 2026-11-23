<?php
interface Shape
{
    public function area(): float;
}

final class Square implements Shape
{
    public function __construct(private float $side)
    {
    }

    public function area(): float
    {
        return $this->side ** 2;
    }
}
