<?php

namespace App\Actions;

trait Runs
{
    public static function run(): void
    {
        (new static())->handle();
    }
}

class Publish
{
    use Runs;

    public function handle(): void
    {
    }
}

class Archive
{
    use Runs;

    public function handle(): void
    {
    }
}
