<?php

declare(strict_types=1);

namespace Opcov\Measure;

use Opcov\Measure\Session\NoneSession;
use Opcov\Measure\Session\OpcovSession;
use Opcov\Measure\Session\PcovSession;
use Opcov\Measure\Session\Session;
use Opcov\Measure\Session\TurbocovSession;
use Opcov\Measure\Session\XdebugSession;

enum Driver: string
{
    case None = 'none';
    case Opcov = 'opcov';
    case Pcov = 'pcov';
    case XdebugLines = 'xdebug-lines';
    case XdebugBranch = 'xdebug-branch';
    case Turbocov = 'turbocov';

    public function label(): string
    {
        return match ($this) {
            self::None => 'no coverage',
            self::Opcov => 'opcov (lines, outcomes, loops)',
            self::Pcov => 'PCOV',
            self::XdebugLines => 'Xdebug (lines)',
            self::XdebugBranch => 'Xdebug (branch)',
            self::Turbocov => 'turbocov',
        };
    }

    public function version(Toolchain $toolchain): string
    {
        $module = match ($this) {
            self::None => null,
            self::Opcov => 'opcov',
            self::Pcov => 'pcov',
            self::XdebugLines, self::XdebugBranch => 'xdebug',
            self::Turbocov => 'turbocov',
        };
        if ($module === null) {
            return 'PHP ' . $toolchain->phpRelease;
        }
        $version = $toolchain->query(sprintf('echo phpversion(%s) ?: "";', var_export($module, true)), $this->iniArguments($toolchain, new Selection([$toolchain->root], [])));
        if ($version === '') {
            throw new \RuntimeException("{$this->label()} did not load into PHP {$toolchain->phpRelease}");
        }
        return "$module $version";
    }

    public function available(Toolchain $toolchain): bool
    {
        return match ($this) {
            self::None => true,
            self::Opcov => true,
            self::Pcov => $toolchain->hasExtension('pcov'),
            self::XdebugLines, self::XdebugBranch => $toolchain->hasExtension('xdebug'),
            self::Turbocov => $toolchain->hasExtension('turbocov'),
        };
    }

    /** @return list<string> */
    public function iniArguments(Toolchain $toolchain, Selection $selection): array
    {
        $settings = match ($this) {
            self::None => [],
            self::Opcov => [
                'extension' => $toolchain->opcov,
                'opcov.enabled' => '1',
                'opcov.collect_paths' => implode(':', $selection->include),
                'opcov.exclude_paths' => implode(':', $selection->exclude),
            ],
            self::Pcov => [
                'extension' => $toolchain->extension('pcov'),
                'pcov.enabled' => '1',
                'pcov.directory' => self::single($selection->include, 'pcov.directory'),
                'pcov.exclude' => $selection->exclude === [] ? '' : $selection->excludePattern(),
            ],
            self::XdebugLines, self::XdebugBranch => [
                'zend_extension' => $toolchain->extension('xdebug'),
                'xdebug.mode' => 'coverage',
            ],
            self::Turbocov => [
                'extension' => $toolchain->extension('turbocov'),
                'turbocov.collect_paths' => implode(':', $selection->include),
                'turbocov.exclude_paths' => implode(':', $selection->exclude),
            ],
        };
        $settings += ['opcache.enable_cli' => '0', 'memory_limit' => '-1'];
        $arguments = [];
        foreach ($settings as $name => $value) {
            $arguments[] = '-d';
            $arguments[] = "$name=$value";
        }
        return $arguments;
    }

    public function session(Selection $selection): Session
    {
        return match ($this) {
            self::None => new NoneSession(),
            self::Opcov => new OpcovSession(),
            self::Pcov => new PcovSession(),
            self::XdebugLines => new XdebugSession($selection, false),
            self::XdebugBranch => new XdebugSession($selection, true),
            self::Turbocov => new TurbocovSession(),
        };
    }

    /** @param list<string> $paths */
    private static function single(array $paths, string $setting): string
    {
        if (count($paths) !== 1) {
            throw new \InvalidArgumentException(sprintf('%s takes exactly one directory, got %d: %s', $setting, count($paths), implode(', ', $paths)));
        }
        return $paths[0];
    }

    public static function parse(string $name): self
    {
        return self::tryFrom($name) ?? throw new \InvalidArgumentException(sprintf(
            'unknown driver %s; expected one of %s',
            $name,
            implode(', ', array_map(static fn (self $driver): string => $driver->value, self::cases())),
        ));
    }
}
