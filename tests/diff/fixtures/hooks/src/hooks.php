<?php

declare(strict_types=1);

final class Temperature
{
    public float $celsius = 0.0 {
        set(float $value) {
            if ($value < -273.15) {
                throw new InvalidArgumentException('below absolute zero');
            }
            $this->celsius = $value;
        }
    }

    public float $fahrenheit {
        get => $this->celsius * 9 / 5 + 32;
        set => $this->celsius = ($value - 32) * 5 / 9;
    }

    public string $label {
        get {
            return match (true) {
                $this->celsius < 0 => 'freezing',
                $this->celsius < 25 => 'mild',
                default => 'hot',
            };
        }
    }
}
