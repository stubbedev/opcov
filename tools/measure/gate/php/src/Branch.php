<?php

declare(strict_types=1);

namespace Opcov\Measure\Gate;

use Opcov\Measure\CorpusProject;
use Opcov\Measure\Filesystem;
use Opcov\Measure\Json;
use Opcov\Measure\Process;
use Opcov\Measure\Toolchain;

final readonly class Branch
{
    private function __construct(
        public CorpusProject $project,
        public string $description,
        public string $base,
        public string $head,
    ) {
    }

    public static function load(string $root): self
    {
        $path = "$root/tools/measure/gate/branch.json";
        $data = Json::read($path);
        foreach (['project', 'description', 'base', 'head'] as $key) {
            if (!is_string($data[$key] ?? null) || $data[$key] === '') {
                throw new \RuntimeException("$path needs a non-empty string '$key'");
            }
        }
        foreach (['base', 'head'] as $key) {
            if (preg_match('~^[0-9a-f]{40}$~', $data[$key]) !== 1) {
                throw new \RuntimeException("$path needs '$key' as a full 40-digit commit hash");
            }
        }
        $project = CorpusProject::named($root, $data['project']);
        if ($project->commit !== $data['head']) {
            throw new \RuntimeException(sprintf('%s: head %s is not the commit %s that tools/measure/corpus.json pins for %s, so the tests would not run the changed code', $path, $data['head'], $project->commit, $project->name));
        }
        return new self($project, $data['description'], $data['base'], $data['head']);
    }

    public function directory(Toolchain $toolchain): string
    {
        return "$toolchain->cache/gate/{$this->project->name}";
    }

    public function diff(Toolchain $toolchain): string
    {
        $directory = $this->directory($toolchain);
        $diff = "$directory/$this->base-$this->head.diff";
        if (is_file($diff)) {
            return $diff;
        }
        $repository = "$directory/history";
        Filesystem::remove($repository);
        Filesystem::ensureDirectory($repository);
        $git = static fn (string ...$arguments): array => [$toolchain->tool('git'), '-C', $repository, ...$arguments];
        $log = "$directory/git.log";
        $this->git($toolchain, $git('init', '--quiet'), $log, 'git init');
        foreach ([$this->base, $this->head] as $commit) {
            $this->git($toolchain, $git('fetch', '--quiet', '--depth=1', "https://github.com/{$this->project->repository}.git", $commit), $log, "git fetch $commit");
        }
        $output = "$directory/diff.log";
        $this->git($toolchain, $git('diff', '--no-color', '--no-ext-diff', '-U0', $this->base, $this->head), $output, 'git diff');
        $contents = file_get_contents($output) ?: throw new \RuntimeException("git diff $this->base $this->head wrote nothing");
        Filesystem::write($diff, $contents);
        Filesystem::remove($repository);
        return $diff;
    }

    /** @param list<string> $command */
    private function git(Toolchain $toolchain, array $command, string $log, string $what): void
    {
        $result = Process::run($toolchain, $command, [], $this->directory($toolchain), $log, 600);
        if ($result->exit !== 0) {
            throw new \RuntimeException(sprintf("%s for %s exited with %d:\n%s", $what, $this->project->name, $result->exit, $result->tail()));
        }
    }
}
