<?php

declare(strict_types=1);

namespace Opcov\Measure\Gate;

use Opcov\Measure\Driver;
use Opcov\Measure\Filesystem;
use Opcov\Measure\Selection;
use Opcov\Measure\Toolchain;

enum Step: string
{
    case None = 'none';
    case Xdebug = 'xdebug';
    case Opcov = 'opcov';

    public function label(): string
    {
        return match ($this) {
            self::None => 'no coverage',
            self::Xdebug => 'Xdebug path coverage, php-code-coverage `--coverage-php --path-coverage`',
            self::Opcov => 'opcov, `opcov-phpunit` launcher and `OpCov\PHPUnit\Extension`',
        };
    }

    /** @return list<string> */
    public function command(Toolchain $toolchain, Workspace $workspace): array
    {
        $phpunit = [$workspace->runner(), '--do-not-cache-result', '--colors=never'];
        $common = ['-d', 'opcache.enable_cli=0', '-d', 'memory_limit=-1'];
        return match ($this) {
            self::None => [$toolchain->php(), ...$common, ...$phpunit],
            self::Xdebug => [
                $toolchain->php(),
                ...$common,
                ...$phpunit,
                '--coverage-php', $workspace->xdebugCoverage(),
                '--path-coverage',
            ],
            self::Opcov => [
                $toolchain->php(),
                "$toolchain->root/php/phpunit/bin/opcov-phpunit",
                '-d', "extension=$toolchain->opcov",
                '-d', 'auto_prepend_file=' . dirname(__DIR__) . '/package-autoload.php',
                ...$common,
                ...$phpunit,
                '--configuration', $workspace->opcovConfiguration(),
            ],
        };
    }

    /** @return array<string, string> */
    public function environment(Toolchain $toolchain, Workspace $workspace): array
    {
        if ($this !== self::Xdebug) {
            return [];
        }
        $directory = "$workspace->results/xdebug-ini";
        $settings = '';
        foreach (array_chunk(Driver::XdebugBranch->iniArguments($toolchain, new Selection([$workspace->project], [])), 2) as [, $setting]) {
            [$name, $value] = explode('=', $setting, 2);
            $settings .= sprintf("%s=\"%s\"\n", $name, $value);
        }
        Filesystem::writeIfChanged("$directory/xdebug.ini", $settings);
        return ['PHP_INI_SCAN_DIR' => trim($toolchain->query('echo getenv("PHP_INI_SCAN_DIR");')) . ":$directory"];
    }

    public function output(Workspace $workspace): ?string
    {
        return match ($this) {
            self::None => null,
            self::Xdebug => $workspace->xdebugCoverage(),
            self::Opcov => $workspace->opcovReport(),
        };
    }
}
