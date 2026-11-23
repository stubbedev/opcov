<?php

declare(strict_types=1);

namespace Opcov\Measure\Parity;

use Opcov\Measure\Json;

final readonly class Allowlist
{
    private const string ISSUE = '~^https://github\.com/stubbedev/opcov/issues/\d+$~';

    /**
     * @param array<string, AllowlistEntry> $entries
     * @param array<string, RuleEntry> $rules
     */
    private function __construct(
        private array $entries,
        private array $rules,
    ) {
    }

    public static function load(string $path): self
    {
        $data = Json::read($path);
        if (!is_array($data['rules'] ?? null) || !array_is_list($data['rules']) || !is_array($data['entries'] ?? null) || !array_is_list($data['entries'])) {
            throw new \RuntimeException("$path must hold an object with 'rules' and 'entries' lists");
        }
        $rules = [];
        foreach ($data['rules'] as $index => $entry) {
            $context = "$path rule $index";
            if (!is_array($entry) || !is_string($entry['rule'] ?? null) || !self::justified($entry)) {
                throw new \RuntimeException("$context needs rule, a non-empty reason and an issue link matching " . self::ISSUE);
            }
            $rule = Rule::tryFrom($entry['rule']) ?? throw new \RuntimeException("$context names unknown rule {$entry['rule']}");
            if (isset($rules[$rule->value])) {
                throw new \RuntimeException("$context repeats rule {$rule->value}");
            }
            $rules[$rule->value] = new RuleEntry($rule, $entry['reason'], $entry['issue']);
        }
        $entries = [];
        foreach ($data['entries'] as $index => $entry) {
            $context = "$path entry $index";
            if (
                !is_array($entry)
                || !is_string($entry['file'] ?? null)
                || !is_int($entry['line'] ?? null)
                || !is_array($entry['drivers'] ?? null)
                || !is_string($entry['aspect'] ?? null)
                || !self::justified($entry)
            ) {
                throw new \RuntimeException("$context needs file, line, drivers, aspect, a non-empty reason and an issue link matching " . self::ISSUE);
            }
            $aspect = Aspect::tryFrom($entry['aspect']) ?? throw new \RuntimeException("$context has unknown aspect {$entry['aspect']}");
            $allowed = new AllowlistEntry($aspect, $entry['file'], $entry['line'], Pair::fromNames(array_values($entry['drivers']), $context), $entry['reason'], $entry['issue']);
            $key = $allowed->key();
            if (isset($entries[$key])) {
                throw new \RuntimeException("$context repeats $key");
            }
            $entries[$key] = $allowed;
        }
        return new self($entries, $rules);
    }

    /** @param array<mixed> $entry */
    private static function justified(array $entry): bool
    {
        return is_string($entry['reason'] ?? null)
            && trim($entry['reason']) !== ''
            && is_string($entry['issue'] ?? null)
            && preg_match(self::ISSUE, $entry['issue']) === 1;
    }

    /**
     * @param list<Divergence> $divergences
     * @param list<string> $labels
     */
    public function judge(array $divergences, array $labels): Verdict
    {
        $explained = [];
        $unexplained = [];
        $used = [];
        $usedRules = [];
        foreach ($divergences as $divergence) {
            $rule = $divergence->rule === null ? null : ($this->rules[$divergence->rule->value] ?? null);
            if ($rule !== null) {
                $explained[] = [$divergence, $rule];
                $usedRules[$rule->rule->value] = true;
                continue;
            }
            $entry = $this->entries[$divergence->key()] ?? null;
            if ($entry === null) {
                $unexplained[] = $divergence;
                continue;
            }
            $explained[] = [$divergence, $entry];
            $used[$entry->key()] = true;
        }
        $stale = array_values(array_filter(
            $this->entries,
            static fn (AllowlistEntry $entry): bool => !isset($used[$entry->key()]) && $entry->coveredBy($labels),
        ));
        foreach ($this->rules as $name => $rule) {
            if (!isset($usedRules[$name])) {
                $stale[] = $rule;
            }
        }
        return new Verdict($explained, $unexplained, $stale);
    }
}
