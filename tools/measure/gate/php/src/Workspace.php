<?php

declare(strict_types=1);

namespace Opcov\Measure\Gate;

use Opcov\Measure\Filesystem;
use Opcov\Measure\Toolchain;

final readonly class Workspace
{
    private const string CONFIGURATION = 'phpunit.opcov-gate.xml';

    private function __construct(
        public string $project,
        public string $results,
        private string $runner,
    ) {
    }

    public static function prepare(Branch $branch, Toolchain $toolchain): self
    {
        $workspace = self::open($branch, $toolchain);
        $source = "$workspace->project/{$branch->project->configuration}";
        Filesystem::writeIfChanged($workspace->opcovConfiguration(), self::derivedConfiguration($source, $workspace->opcovReport()));
        return $workspace;
    }

    public static function open(Branch $branch, Toolchain $toolchain): self
    {
        return new self($branch->project->fetch($toolchain), $branch->directory($toolchain), $branch->project->runner);
    }

    private static function derivedConfiguration(string $source, string $report): string
    {
        $document = new \DOMDocument();
        if (!@$document->load($source)) {
            throw new \RuntimeException("cannot parse the PHPUnit configuration $source");
        }
        $root = $document->documentElement ?? throw new \RuntimeException("$source has no root element");
        $extensions = $root->getElementsByTagName('extensions')->item(0) ?? $root->appendChild($document->createElement('extensions'));
        $bootstrap = $document->createElement('bootstrap');
        $bootstrap->setAttribute('class', 'OpCov\PHPUnit\Extension');
        $parameter = $document->createElement('parameter');
        $parameter->setAttribute('name', 'output');
        $parameter->setAttribute('value', $report);
        $bootstrap->appendChild($parameter);
        $extensions->appendChild($bootstrap);
        return $document->saveXML() ?: throw new \RuntimeException("cannot serialize the PHPUnit configuration derived from $source");
    }

    public function runner(): string
    {
        return "$this->project/$this->runner";
    }

    public function opcovConfiguration(): string
    {
        return "$this->project/" . self::CONFIGURATION;
    }

    public function opcovReport(): string
    {
        return "$this->results/opcov.json";
    }

    public function xdebugCoverage(): string
    {
        return "$this->results/xdebug.cov";
    }

    public function opcovGate(): string
    {
        return "$this->results/gate-opcov.json";
    }

    public function referenceGate(): string
    {
        return "$this->results/gate-xdebug.json";
    }

    public function timings(): string
    {
        return "$this->results/timings.json";
    }

    public function log(string $name): string
    {
        return "$this->results/logs/$name.log";
    }
}
