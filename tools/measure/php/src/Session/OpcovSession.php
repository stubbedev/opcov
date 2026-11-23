<?php

declare(strict_types=1);

namespace Opcov\Measure\Session;

use Opcov\Measure\Coverage;
use Opcov\Measure\Decision;
use Opcov\Measure\Loop;
use Opcov\Measure\Outcome;

final class OpcovSession implements Session
{
    /** @var array<string, array<string, array<string, true>>> */
    private array $outcomeHits = [];

    /** @var array<string, array<string, array<string, array{zero: bool, second: bool}>>> */
    private array $loopHits = [];

    public function __construct()
    {
        \opcov\ensure_can_collect();
    }

    public function start(): void
    {
        \opcov\start();
    }

    public function stop(string $test): mixed
    {
        return \opcov\stop($test);
    }

    public function rearm(): void
    {
        \opcov\reset();
    }

    public function collect(string $test, Coverage $coverage): void
    {
        $this->record($this->stop($test), $coverage);
    }

    private function record(mixed $raw, Coverage $coverage): void
    {
        if (!is_array($raw)) {
            throw new \UnexpectedValueException('opcov\stop() returned ' . get_debug_type($raw));
        }
        foreach ($raw as $file => $entry) {
            $file = (string) $file;
            if (!is_array($entry) || !is_array($entry['lines'] ?? null) || !is_array($entry['functions'] ?? null)) {
                throw new \UnexpectedValueException("opcov\\stop() returned no 'lines' and 'functions' for $file");
            }
            $coverage->addLines($file, array_map(static fn (mixed $hit): bool => $hit === true ? true : throw new \UnexpectedValueException("opcov\\stop() reported a line of $file as " . var_export($hit, true)), $entry['lines']));
            foreach ($entry['functions'] as $function => $hits) {
                $function = (string) $function;
                if (!is_array($hits) || !is_array($hits['outcomes'] ?? null) || !is_array($hits['loops'] ?? null)) {
                    throw new \UnexpectedValueException("opcov\\stop() returned no 'outcomes' and 'loops' for $file $function");
                }
                foreach (array_keys($hits['outcomes']) as $id) {
                    $this->outcomeHits[$file][$function][(string) $id] = true;
                }
                foreach ($hits['loops'] as $id => $flags) {
                    $known = $this->loopHits[$file][$function][(string) $id] ?? ['zero' => false, 'second' => false];
                    $this->loopHits[$file][$function][(string) $id] = [
                        'zero' => $known['zero'] || ($flags['zero'] ?? false) === true,
                        'second' => $known['second'] || ($flags['second'] ?? false) === true,
                    ];
                }
            }
        }
    }

    public function writeReport(string $path, string $root): int
    {
        \opcov\write_report($path, ['root' => $root]);
        clearstatcache(true, $path);
        $size = filesize($path);
        if ($size === false) {
            throw new \RuntimeException("opcov\\write_report() wrote no $path");
        }
        return $size;
    }

    public function complete(Coverage $coverage): void
    {
        $files = \opcov\get_status()['selected_files'];
        $models = \opcov\executable($files);
        foreach ($models as $file => $model) {
            $coverage->addLines($file, array_map(static fn (): bool => false, $model['lines']));
            $cfgs = \opcov\get_debug_info($file)['functions'];
            foreach ($model['functions'] as $key => $function) {
                $context = "$file $key";
                $cfg = $cfgs[$key]['function'] ?? throw new \UnexpectedValueException("opcov\\executable() lists $context, which opcov\\get_debug_info() does not");
                $coverage->addDecisions($file, $key, self::decisions($cfg, $function['outcomes'], $this->outcomeHits[$file][$key] ?? [], $context));
                $coverage->addLoops($file, $key, self::loops($function['loops'], $this->loopHits[$file][$key] ?? []));
            }
        }
    }

    /**
     * @param array<string, mixed> $cfg
     * @param list<array<string, mixed>> $outcomes
     * @param array<string, true> $hits
     * @return list<Decision>
     */
    private static function decisions(array $cfg, array $outcomes, array $hits, string $context): array
    {
        $blocks = [];
        foreach ($cfg['blocks'] as $block) {
            $blocks[$block['id']] = $block;
        }
        $ids = [];
        foreach ($outcomes as $outcome) {
            $ids[$outcome['site']][self::kind($outcome['kind'])][] = $outcome['id'];
        }
        $outgoing = [];
        foreach ($cfg['edges'] as $edge) {
            $outgoing[$edge['from']][] = $edge;
        }
        uksort($outgoing, static fn (int $left, int $right): int => $blocks[$left]['op_start'] <=> $blocks[$right]['op_start']);
        $decisions = [];
        foreach ($outgoing as $from => $edges) {
            if (count($edges) < 2) {
                continue;
            }
            $site = $blocks[$from]['op_end'];
            $claimed = [];
            $resolved = [];
            foreach ($edges as $edge) {
                $kind = self::kind($edge['kind']);
                $ordinal = $claimed[$kind] ?? 0;
                $claimed[$kind] = $ordinal + 1;
                $id = $ids[$site][$kind][$ordinal] ?? null;
                $resolved[] = [$edge, $kind, $id === null ? null : isset($hits[$id])];
            }
            foreach ($ids[$site] ?? [] as $kind => $list) {
                if (count($list) > ($claimed[$kind] ?? 0)) {
                    throw new \UnexpectedValueException(sprintf('%s: opcov\executable() lists %d %s outcomes at opcode %d, the CFG has %d such edges', $context, count($list), $kind, $site, $claimed[$kind] ?? 0));
                }
            }
            usort($resolved, static fn (array $left, array $right): int => $blocks[$left[0]['to']]['op_start'] <=> $blocks[$right[0]['to']]['op_start']);
            $decisions[] = new Decision(
                $blocks[$from]['ops'][array_key_last($blocks[$from]['ops'])]['line'],
                array_map(static fn (array $entry): Outcome => new Outcome($blocks[$entry[0]['to']]['ops'][0]['line'], $entry[1], $entry[2]), $resolved),
            );
        }
        return $decisions;
    }

    /**
     * @param list<array<string, mixed>> $loops
     * @param array<string, array{zero: bool, second: bool}> $hits
     * @return list<Loop>
     */
    private static function loops(array $loops, array $hits): array
    {
        return array_map(
            static fn (array $loop): Loop => new Loop(
                $loop['line'],
                $loop['kind'],
                $loop['zero'] === null ? null : ($hits[$loop['id']]['zero'] ?? false),
                $hits[$loop['id']]['second'] ?? false,
            ),
            $loops,
        );
    }

    private static function kind(mixed $kind): string
    {
        if (is_string($kind)) {
            return $kind;
        }
        if (is_array($kind) && count($kind) === 1) {
            return sprintf('%s(%s)', array_key_first($kind), json_encode(reset($kind), JSON_THROW_ON_ERROR));
        }
        throw new \UnexpectedValueException('opcov returned an edge kind of shape ' . json_encode($kind));
    }
}
