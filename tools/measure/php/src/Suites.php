<?php

declare(strict_types=1);

namespace Opcov\Measure;

final readonly class Suites
{
    public function __construct(private Toolchain $toolchain)
    {
    }

    /** @return list<Target> */
    public function resolve(string $suite): array
    {
        if ($suite === 'golden') {
            return $this->golden();
        }
        if ($suite === 'fixtures') {
            return $this->fixtures();
        }
        if ($suite === 'corpus') {
            return array_values(array_map(
                fn (CorpusProject $project): Target => CorpusTarget::prepare($project, $this->toolchain),
                CorpusProject::all($this->toolchain->root),
            ));
        }
        if (str_starts_with($suite, 'corpus:')) {
            return [CorpusTarget::prepare(CorpusProject::named($this->toolchain->root, substr($suite, 7)), $this->toolchain)];
        }
        throw new \InvalidArgumentException("unknown suite $suite; expected golden, fixtures, corpus or corpus:<name>");
    }

    /** @return list<Target> */
    private function golden(): array
    {
        $root = $this->toolchain->root;
        $goldens = glob("$root/crates/opcov-core/tests/golden/cfg/*.json") ?: throw new \RuntimeException("no golden CFG fixtures under $root/crates/opcov-core/tests/golden/cfg");
        $runners = glob("$root/tests/diff/golden/*.php") ?: [];
        $runners = [...$runners, ...(glob("$root/tests/diff/golden/*.expect.json") ?: [])];
        $names = array_map(static fn (string $path): string => basename($path, '.json'), $goldens);
        $stale = array_diff(array_map(static fn (string $path): string => preg_replace('~\.(expect\.json|php)$~', '', basename($path)), $runners), $names);
        if ($stale !== []) {
            throw new \RuntimeException('tests/diff/golden has runners for golden fixtures that no longer exist: ' . implode(', ', $stale));
        }
        $targets = [];
        foreach ($goldens as $golden) {
            $name = basename($golden, '.json');
            $runner = "$root/tests/diff/golden/$name.php";
            if (!is_file($runner)) {
                throw new \RuntimeException("golden fixture $golden has no runner $runner that calls its function");
            }
            $source = Json::read($golden)['source'] ?? null;
            if (!is_string($source)) {
                throw new \RuntimeException("$golden has no 'source'");
            }
            $sources = "{$this->toolchain->cache}/golden/$name";
            Filesystem::writeIfChanged("$sources/$name.php", $source);
            $targets[] = new StandaloneTarget("golden/$name", Workload::open($sources, $runner), "$root/tests/diff/golden/$name.expect.json");
        }
        return $targets;
    }

    /** @return list<Target> */
    private function fixtures(): array
    {
        $directories = glob("{$this->toolchain->root}/tests/diff/fixtures/*", GLOB_ONLYDIR) ?: throw new \RuntimeException('no fixtures under tests/diff/fixtures');
        return array_map(
            static fn (string $directory): Target => new StandaloneTarget('fixtures/' . basename($directory), Workload::open("$directory/src", "$directory/run.php"), "$directory/expect.json"),
            $directories,
        );
    }
}
