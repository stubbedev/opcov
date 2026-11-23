<?php

declare(strict_types=1);

namespace Opcov\Measure\Gate;

use PhpParser\Node;
use PhpParser\NodeTraverser;
use PhpParser\NodeVisitor\NameResolver;
use PhpParser\NodeVisitorAbstract;
use PhpParser\ParserFactory;

final readonly class FunctionMap
{
    /** @param list<FunctionSpan> $functions */
    private function __construct(public array $functions)
    {
    }

    public static function parse(string $path, string $relative): self
    {
        $code = @file_get_contents($path);
        if ($code === false) {
            throw new \RuntimeException("cannot read $path");
        }
        try {
            $statements = (new ParserFactory())->createForHostVersion()->parse($code) ?? throw new \RuntimeException('the parser returned no statements');
        } catch (\Throwable $e) {
            throw new \RuntimeException("cannot parse $path", 0, $e);
        }
        $collector = new class ($relative) extends NodeVisitorAbstract {
            /** @var list<FunctionSpan> */
            public array $functions = [];

            /** @var list<string> */
            private array $classes = [];

            private int $depth = 0;

            public function __construct(private readonly string $file)
            {
            }

            public function enterNode(Node $node): null
            {
                if ($node instanceof Node\Stmt\ClassLike) {
                    $this->classes[] = $node->name === null
                        ? sprintf('class@anonymous@%s:%d-%d', $this->file, $node->getStartLine(), $node->getEndLine())
                        : $node->namespacedName?->toString() ?? throw new \RuntimeException("{$this->file}: class {$node->name} has no resolved name");
                    return null;
                }
                $key = match (true) {
                    $node instanceof Node\Stmt\Function_ => $node->namespacedName?->toString() ?? throw new \RuntimeException("{$this->file}: function {$node->name} has no resolved name"),
                    $node instanceof Node\Stmt\ClassMethod && $node->stmts !== null => $this->currentClass() . '::' . $node->name->toString(),
                    $node instanceof Node\Expr\Closure, $node instanceof Node\Expr\ArrowFunction => sprintf('{closure}@%s:%d-%d', $this->file, $node->getStartLine(), $node->getEndLine()),
                    default => null,
                };
                if ($key !== null) {
                    $this->functions[] = new FunctionSpan($key, $node->getStartLine(), $node->getEndLine(), $node instanceof Node\Expr\Closure || $node instanceof Node\Expr\ArrowFunction);
                }
                return null;
            }

            public function leaveNode(Node $node): null
            {
                if ($node instanceof Node\Stmt\ClassLike) {
                    array_pop($this->classes);
                }
                return null;
            }

            private function currentClass(): string
            {
                return $this->classes[array_key_last($this->classes)] ?? throw new \RuntimeException("{$this->file}: a method outside a class");
            }
        };
        $traverser = new NodeTraverser(new NameResolver(), $collector);
        $traverser->traverse($statements);
        $main = new FunctionSpan("{main}@$relative", 1, substr_count($code, "\n") + 1, false);
        return new self([$main, ...$collector->functions]);
    }

    public function ownerOfLine(int $line): ?FunctionSpan
    {
        return self::innermost(array_filter($this->functions, static fn (FunctionSpan $function): bool => $function->contains($line)));
    }

    public function ownerOfGapBefore(int $line): ?FunctionSpan
    {
        return self::innermost(array_filter($this->functions, static fn (FunctionSpan $function): bool => $function->containsGapBefore($line)));
    }

    /**
     * @param list<int> $lines
     * @param list<int> $gapsBefore
     * @return list<FunctionSpan>
     */
    public function select(array $lines, array $gapsBefore): array
    {
        $selected = [];
        foreach ($lines as $line) {
            $owner = $this->ownerOfLine($line);
            if ($owner !== null) {
                $selected[$owner->key] = $owner;
            }
        }
        foreach ($gapsBefore as $line) {
            $owner = $this->ownerOfGapBefore($line);
            if ($owner !== null) {
                $selected[$owner->key] = $owner;
            }
        }
        foreach ($this->functions as $function) {
            if ($function->nested && $this->hasSelectedAncestor($function, $selected)) {
                $selected[$function->key] = $function;
            }
        }
        ksort($selected, SORT_STRING);
        return array_values($selected);
    }

    /** @return list<int> */
    public function ownedLines(FunctionSpan $function, int $first, int $last): array
    {
        $owned = [];
        for ($line = max($first, $function->start); $line <= min($last, $function->end); $line++) {
            if ($this->ownerOfLine($line) === $function) {
                $owned[] = $line;
            }
        }
        return $owned;
    }

    /** @param array<string, FunctionSpan> $selected */
    private function hasSelectedAncestor(FunctionSpan $function, array $selected): bool
    {
        for ($parent = $this->parentOf($function); $parent !== null; $parent = $this->parentOf($parent)) {
            if (isset($selected[$parent->key])) {
                return true;
            }
            if (!$parent->nested) {
                return false;
            }
        }
        return false;
    }

    private function parentOf(FunctionSpan $function): ?FunctionSpan
    {
        return self::innermost(array_filter($this->functions, static fn (FunctionSpan $outer): bool => $outer->encloses($function)));
    }

    /** @param array<int, FunctionSpan> $candidates */
    private static function innermost(array $candidates): ?FunctionSpan
    {
        $best = null;
        foreach ($candidates as $candidate) {
            if ($best === null || $candidate->narrowerThan($best)) {
                $best = $candidate;
            }
        }
        return $best;
    }
}
