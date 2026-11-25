<?php

namespace App\Uses;

use App\Services\InheritingReport;
use App\Services\OverridingReport;
use App\Services\ProvidesReport;
use App\Services\Report;
use App\Services\Unrelated;

final class Receivers
{
    public function run(Report $report, ?Report $maybe, InheritingReport $inheriting, OverridingReport $overriding, ProvidesReport $provider, Unrelated $unrelated, mixed $unknown): void
    {
        $report->render();
        $maybe?->render();
        $inheriting->render();
        $overriding->render();
        $provider->render();
        $unrelated->render();
        $unknown->render();
        new Report();
        echo Report::LIMIT, $report->title;
    }
}
