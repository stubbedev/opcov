<?php

declare(strict_types=1);

namespace Opcov\Measure;

final readonly class CorpusProject
{
    private const string READY = '.opcov-measure-ready';

    private const string LOCK_PLATFORM_PHP = '8.4.27';

    /**
     * @param list<string> $measure
     * @param list<string> $exclude
     * @param array<string, string> $requireDev
     * @param array<string, string> $environment
     * @param list<string> $arguments
     */
    private function __construct(
        public string $name,
        public string $repository,
        public string $version,
        public string $commit,
        public array $measure,
        public array $exclude,
        public string $runner,
        public string $configuration,
        public array $requireDev,
        public array $environment,
        public array $arguments,
        public Driver $xdebug,
    ) {
    }

    /** @return array<string, self> */
    public static function all(string $root): array
    {
        $path = self::definitions($root);
        $projects = [];
        foreach (Json::read($path) as $index => $entry) {
            $project = self::fromArray($entry, "$path entry $index");
            $projects[$project->name] = $project;
        }
        return $projects;
    }

    public static function named(string $root, string $name): self
    {
        $projects = self::all($root);
        return $projects[$name] ?? throw new \InvalidArgumentException(sprintf(
            'unknown corpus project %s; %s defines %s',
            $name,
            self::definitions($root),
            implode(', ', array_keys($projects)),
        ));
    }

    private static function definitions(string $root): string
    {
        return "$root/tools/measure/corpus.json";
    }

    private static function fromArray(mixed $entry, string $context): self
    {
        if (!is_array($entry)) {
            throw new \RuntimeException("$context is not an object");
        }
        foreach (['name', 'repository', 'version', 'commit', 'runner', 'configuration'] as $key) {
            if (!is_string($entry[$key] ?? null) || $entry[$key] === '') {
                throw new \RuntimeException("$context needs a non-empty string '$key'");
            }
        }
        if (preg_match('~^[0-9a-f]{40}$~', $entry['commit']) !== 1) {
            throw new \RuntimeException("$context needs a full 40-digit commit hash");
        }
        $list = static fn (string $key): array => is_array($entry[$key] ?? null) && array_is_list($entry[$key]) && array_filter($entry[$key], 'is_string') === $entry[$key]
            ? $entry[$key]
            : throw new \RuntimeException("$context needs '$key' as a list of strings");
        $map = static fn (string $key): array => is_array($entry[$key] ?? null) && array_filter($entry[$key], 'is_string') === $entry[$key] && array_filter(array_keys($entry[$key]), 'is_string') === array_keys($entry[$key])
            ? $entry[$key]
            : throw new \RuntimeException("$context needs '$key' as an object of strings");
        return new self(
            $entry['name'],
            $entry['repository'],
            $entry['version'],
            $entry['commit'],
            $list('measure'),
            $list('exclude'),
            $entry['runner'],
            $entry['configuration'],
            $map('require-dev'),
            $map('environment'),
            $list('arguments'),
            match ($entry['xdebug'] ?? null) {
                'branch' => Driver::XdebugBranch,
                'lines' => Driver::XdebugLines,
                default => throw new \RuntimeException("$context needs 'xdebug': 'branch' or 'lines'"),
            },
        );
    }

    public function directory(Toolchain $toolchain): string
    {
        return "$toolchain->cache/corpus/$this->name-$this->commit";
    }

    public function lockFile(Toolchain $toolchain): string
    {
        return "$toolchain->root/tools/measure/corpus/$this->name.lock";
    }

    public function fetch(Toolchain $toolchain): string
    {
        $directory = $this->directory($toolchain);
        $lock = $this->lockFile($toolchain);
        $lockHash = @hash_file('sha256', $lock);
        if ($lockHash === false) {
            throw new \RuntimeException("corpus project $this->name has no stored lock $lock; create it with `tools/measure/measure lock --suite $this->name`");
        }
        if (@file_get_contents("$directory/" . self::READY) === $lockHash) {
            return $directory;
        }
        $this->checkout($toolchain, $directory);
        if (!copy($lock, "$directory/composer.lock")) {
            throw new \RuntimeException("cannot copy $lock into $directory");
        }
        $this->composer($toolchain, $directory, ['install']);
        self::markReady($directory, $lockHash);
        return $directory;
    }

    public function lock(Toolchain $toolchain): void
    {
        $directory = $this->directory($toolchain);
        $this->checkout($toolchain, $directory);
        $this->composer($toolchain, $directory, ['update']);
        $contents = file_get_contents("$directory/composer.lock");
        if ($contents === false) {
            throw new \RuntimeException("composer update wrote no $directory/composer.lock");
        }
        Filesystem::write($this->lockFile($toolchain), $contents);
        self::markReady($directory, hash('sha256', $contents));
    }

    private static function markReady(string $directory, string $lockHash): void
    {
        if (file_put_contents("$directory/" . self::READY, $lockHash) !== strlen($lockHash)) {
            throw new \RuntimeException("cannot mark $directory as installed");
        }
    }

    private function checkout(Toolchain $toolchain, string $directory): void
    {
        Filesystem::remove($directory);
        Filesystem::ensureDirectory($directory);
        $git = static fn (string ...$arguments): array => [$toolchain->tool('git'), '-C', $directory, '-c', 'advice.detachedHead=false', ...$arguments];
        $this->run($toolchain, $directory, $git('init', '--quiet'), 'git init');
        $this->run($toolchain, $directory, $git('fetch', '--quiet', '--depth=1', "https://github.com/$this->repository.git", $this->commit), "git fetch $this->commit");
        $this->run($toolchain, $directory, $git('checkout', '--quiet', $this->commit), "git checkout $this->commit");
        Filesystem::remove("$directory/.git");
        $this->composer($toolchain, $directory, ['config', 'platform.php', self::LOCK_PLATFORM_PHP]);
        foreach ($this->requireDev as $package => $constraint) {
            $this->composer($toolchain, $directory, ['require', '--dev', '--no-update', "$package:$constraint"]);
        }
    }

    /** @param list<string> $arguments */
    private function composer(Toolchain $toolchain, string $directory, array $arguments): void
    {
        $this->run(
            $toolchain,
            $directory,
            [$toolchain->tool('composer'), ...$arguments, '--no-interaction', ...(in_array($arguments[0], ['install', 'update'], true) ? ['--prefer-dist', '--no-progress'] : [])],
            'composer ' . implode(' ', $arguments),
        );
    }

    /** @param list<string> $command */
    private function run(Toolchain $toolchain, string $directory, array $command, string $what): void
    {
        $result = Process::run(
            $toolchain,
            $command,
            ['COMPOSER_HOME' => "$toolchain->cache/composer", 'COMPOSER_NO_AUDIT' => '1'],
            $directory,
            "$toolchain->cache/logs/fetch-$this->name.log",
            1800,
        );
        if ($result->exit !== 0) {
            throw new \RuntimeException(sprintf("%s for corpus project %s exited with %d:\n%s", $what, $this->name, $result->exit, $result->tail()));
        }
    }
}
