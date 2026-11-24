<?php

declare(strict_types=1);

namespace Opcov\Measure;

final readonly class Arguments
{
    /**
     * @param array<string, list<string>> $options
     * @param array<string, true> $flags
     */
    private function __construct(
        private array $options,
        private array $flags,
    ) {
    }

    /**
     * @param list<string> $argv
     * @param list<string> $valued
     * @param list<string> $switches
     */
    public static function parse(array $argv, array $valued, array $switches): self
    {
        $command = $argv[0] ?? throw new \InvalidArgumentException('missing command');
        $options = [];
        $flags = [];
        for ($i = 1; $i < count($argv); $i++) {
            $name = $argv[$i];
            if (in_array($name, $switches, true)) {
                $flags[$name] = true;
                continue;
            }
            if (!in_array($name, $valued, true)) {
                throw new \InvalidArgumentException("unknown option $name for $command; options: " . implode(' ', [...$valued, ...$switches]));
            }
            $value = $argv[++$i] ?? throw new \InvalidArgumentException("option $name needs a value");
            $options[$name][] = $value;
        }
        return new self($options, $flags);
    }

    public function required(string $name): string
    {
        $values = $this->options[$name] ?? throw new \InvalidArgumentException("missing required option $name");
        if (count($values) !== 1) {
            throw new \InvalidArgumentException("option $name was given " . count($values) . ' times');
        }
        return $values[0];
    }

    public function optional(string $name, string $default): string
    {
        return isset($this->options[$name]) ? $this->required($name) : $default;
    }

    public function positiveInt(string $name, int $default): int
    {
        $value = $this->optional($name, (string) $default);
        if (preg_match('~^[1-9]\d*$~', $value) !== 1) {
            throw new \InvalidArgumentException("option $name needs a positive integer, got $value");
        }
        return (int) $value;
    }

    /**
     * @param list<string> $default
     * @return list<string>
     */
    public function all(string $name, array $default): array
    {
        return $this->options[$name] ?? $default;
    }

    public function flag(string $name): bool
    {
        return isset($this->flags[$name]);
    }
}
