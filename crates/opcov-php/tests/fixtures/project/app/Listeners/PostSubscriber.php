<?php

namespace App\Listeners;

use App\Events\PostPublished;

class PostSubscriber
{
    public function subscribe(): array
    {
        return [PostPublished::class => 'onPublished'];
    }

    public function onPublished(PostPublished $event): void
    {
    }
}
