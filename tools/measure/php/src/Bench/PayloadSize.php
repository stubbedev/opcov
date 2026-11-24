<?php

declare(strict_types=1);

namespace Opcov\Measure\Bench;

use Opcov\Measure\ReportSize;

final readonly class PayloadSize implements Result
{
    public function __construct(
        public int $tests,
        public int $bytes,
        public ReportSize $report,
    ) {
    }

    public function covers(int $timeoutMinutes): bool
    {
        return true;
    }

    public function accepts(Step $step): bool
    {
        return $step === Step::Payload;
    }

    /** @return array{tests: int, payload_bytes: int, report: mixed} */
    public function toArray(): array
    {
        return ['tests' => $this->tests, 'payload_bytes' => $this->bytes, 'report' => $this->report->toArray()];
    }
}
