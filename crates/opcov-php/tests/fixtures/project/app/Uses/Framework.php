<?php

namespace App\Uses;

use App\Actions\Archive;
use App\Actions\Publish;
use App\Events\PostPublished;
use App\Jobs\PlainHandler;
use App\Jobs\SendDigest;

final class Framework
{
    public function run(): void
    {
        Publish::run();
        Archive::run();
        SendDigest::dispatch();
        dispatch(new SendDigest());
        new PlainHandler();
        PostPublished::dispatch();
        event(new PostPublished());
    }
}
