<?php

declare(strict_types=1);

final class Address
{
    public function __construct(public ?string $city = null)
    {
    }

    public function upper(): ?string
    {
        return $this->city === null ? null : strtoupper($this->city);
    }
}

final class Person
{
    public function __construct(public ?Address $address = null)
    {
    }
}

function city(?Person $person): string
{
    return $person?->address?->upper() ?? 'unknown';
}

function cityLength(?Person $person): int
{
    return strlen($person
        ?->address
        ?->city ?? '');
}
