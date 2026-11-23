<?php

declare(strict_types=1);

namespace Opcov\Measure;

use Opcov\Measure\Session\Session;

final class Run
{
    private readonly Session $session;

    private readonly Coverage $coverage;

    private bool $collecting = false;

    private int $tests = 0;

    private int $payload = 0;

    private string $test = '';

    public function __construct(private readonly Config $config)
    {
        $this->session = $config->driver->session($config->selection);
        $this->coverage = new Coverage();
    }

    public function beginTest(string $test): void
    {
        if ($this->collecting) {
            $this->endTest();
        }
        $this->test = $test;
        $this->collecting = true;
        $this->session->start();
    }

    public function endTest(): void
    {
        if (!$this->collecting) {
            return;
        }
        $this->collecting = false;
        $this->tests++;
        match ($this->config->mode) {
            Mode::Coverage => $this->session->collect($this->test, $this->coverage),
            Mode::Payload => $this->payload += strlen(serialize($this->session->stop($this->test))),
            Mode::Discard => $this->session->stop($this->test),
        };
        $this->session->rearm();
    }

    public function finish(): void
    {
        $this->endTest();
        $result = ['driver' => $this->config->driver->value, 'php' => PHP_VERSION, 'tests' => $this->tests];
        match ($this->config->mode) {
            Mode::Coverage => $result += $this->completedCoverage(),
            Mode::Payload => $result += ['payload_bytes' => $this->payload, 'report_bytes' => $this->session->writeReport("{$this->config->output}.report.json", $this->config->root)],
            Mode::Discard => null,
        };
        Json::write($this->config->output, $result);
    }

    /** @return array<string, mixed> */
    private function completedCoverage(): array
    {
        $this->session->complete($this->coverage);
        return $this->coverage->relativeTo($this->config->root)->toArray();
    }
}
