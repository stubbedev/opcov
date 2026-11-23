<?php

declare(strict_types=1);

namespace Opcov\Measure;

final readonly class CorpusTarget implements Target
{
    private const string CONFIGURATION = 'phpunit.opcov-measure.xml';

    private function __construct(
        private CorpusProject $project,
        private string $directory,
    ) {
    }

    public static function prepare(CorpusProject $project, Toolchain $toolchain): self
    {
        $directory = $project->fetch($toolchain);
        Filesystem::writeIfChanged("$directory/" . self::CONFIGURATION, self::configuration("$directory/$project->configuration"));
        return new self($project, $directory);
    }

    private static function configuration(string $source): string
    {
        $document = new \DOMDocument();
        if (!@$document->load($source)) {
            throw new \RuntimeException("cannot parse the PHPUnit configuration $source");
        }
        $root = $document->documentElement ?? throw new \RuntimeException("$source has no root element");
        $extensions = $root->getElementsByTagName('extensions')->item(0);
        if ($extensions === null) {
            $extensions = $root->appendChild($document->createElement('extensions'));
        }
        $bootstrap = $document->createElement('bootstrap');
        $bootstrap->setAttribute('class', PhpUnitExtension::class);
        $extensions->appendChild($bootstrap);
        return $document->saveXML() ?: throw new \RuntimeException("cannot serialize the PHPUnit configuration derived from $source");
    }

    public function label(): string
    {
        return 'corpus/' . $this->project->name;
    }

    public function root(): string
    {
        return $this->directory;
    }

    public function selection(): Selection
    {
        $absolute = fn (string $path): string => realpath("$this->directory/$path") ?: throw new \RuntimeException("corpus project {$this->project->name} has no path $path");
        return new Selection(array_map($absolute, $this->project->measure), array_map($absolute, $this->project->exclude));
    }

    public function environment(): array
    {
        return $this->project->environment;
    }

    public function cwd(): string
    {
        return $this->directory;
    }

    public function script(): array
    {
        return [
            '-d', 'auto_prepend_file=' . dirname(__DIR__) . '/autoload.php',
            "$this->directory/{$this->project->runner}",
            '--configuration', "$this->directory/" . self::CONFIGURATION,
            '--do-not-cache-result',
            ...$this->project->arguments,
        ];
    }

    public function xdebug(): Driver
    {
        return $this->project->xdebug;
    }

    public function expectations(): ?string
    {
        return null;
    }

    public function acceptedExits(): array
    {
        return [0, 1, 2];
    }
}
