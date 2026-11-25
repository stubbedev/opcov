<?php

namespace App\Console;

use Illuminate\Console\Command;

class Reindex extends Command
{
    protected $signature = 'app:reindex {--all}';

    public function handle(): void
    {
    }
}
