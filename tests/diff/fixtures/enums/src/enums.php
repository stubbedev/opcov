<?php

declare(strict_types=1);

interface HasColor
{
    public function color(): string;
}

enum Suit: string implements HasColor
{
    case Hearts = 'H';
    case Diamonds = 'D';
    case Clubs = 'C';
    case Spades = 'S';

    public const Suit Wild = self::Spades;

    public function color(): string
    {
        return match ($this) {
            self::Hearts, self::Diamonds => 'red',
            self::Clubs, self::Spades => 'black',
        };
    }

    public static function fromChar(string $char): self
    {
        return self::tryFrom(strtoupper($char)) ?? self::Wild;
    }

    public function isRed(): bool
    {
        return $this->color() === 'red';
    }
}
