<?php

declare(strict_types=1);

namespace Opcov\Measure;

final readonly class Toolchain
{
    private function __construct(
        public string $root,
        public string $environment,
        public string $opcov,
        public string $cache,
        public string $phpRelease,
    ) {
    }

    public static function open(string $environment, string $opcov, string $root): self
    {
        $resolved = realpath($environment);
        if ($resolved === false || !is_file("$resolved/bin/php")) {
            throw new \InvalidArgumentException("measurement environment $environment has no bin/php; build it with tools/measure/env.nix");
        }
        $extension = realpath($opcov);
        if ($extension === false || !is_file($extension)) {
            throw new \InvalidArgumentException("opcov extension $opcov does not exist");
        }
        return new self($root, $resolved, $extension, "$root/target/measure-cache", self::evaluate("$resolved/bin/php", 'echo PHP_VERSION;'));
    }

    public function phpVersion(): string
    {
        return implode('.', array_slice(explode('.', $this->phpRelease), 0, 2));
    }

    /** @param list<string> $arguments */
    public function query(string $code, array $arguments = []): string
    {
        return self::evaluate($this->php(), $code, $arguments);
    }

    /** @param list<string> $arguments */
    private static function evaluate(string $php, string $code, array $arguments = []): string
    {
        $process = proc_open([$php, ...$arguments, '-r', $code], [0 => ['file', '/dev/null', 'r'], 1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes, null, Process::cleanEnvironment());
        if ($process === false) {
            throw new \RuntimeException("cannot start $php");
        }
        $output = stream_get_contents($pipes[1]);
        $errors = stream_get_contents($pipes[2]);
        fclose($pipes[1]);
        fclose($pipes[2]);
        $exit = proc_close($process);
        if ($exit !== 0 || $output === false) {
            throw new \RuntimeException(sprintf('%s %s -r %s exited with %d: %s', $php, implode(' ', $arguments), $code, $exit, $errors));
        }
        return $output;
    }

    public function php(): string
    {
        return "$this->environment/bin/php";
    }

    public function tool(string $name): string
    {
        return "$this->environment/bin/$name";
    }

    public function hasExtension(string $name): bool
    {
        return is_file($this->extensionPath($name));
    }

    public function extension(string $name): string
    {
        $path = $this->extensionPath($name);
        if (!is_file($path)) {
            throw new \RuntimeException("the measurement environment $this->environment has no $name extension for this PHP version");
        }
        return $path;
    }

    private function extensionPath(string $name): string
    {
        return "$this->environment/ext/$name.so";
    }
}
