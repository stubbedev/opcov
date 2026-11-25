<?php

namespace App\Uses;

use App\Services\Report;

class Lookalike
{
    public function run(Report $report): void
    {
        $report->renderer();
        $report->prerender();
        $report->render_all();
    }
}
