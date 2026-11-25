<?php

namespace App\Uses;

use App\Console\Reindex;
use App\Facades\Time;
use Artisan;
use Illuminate\Console\Command;

final class Facades
{
    public function run(Command $command): void
    {
        Time::now();
        Artisan::call('app:reindex');
        Artisan::call(Reindex::class);
        $command->call('app:reindex');
        Artisan::call('other:command');
    }
}
