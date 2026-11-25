<?php

namespace App\Listeners;

use App\Events\CommentCreated;

class CommentSubscriber
{
    public function subscribe(): array
    {
        return [CommentCreated::class => 'onCreated'];
    }

    public function onCreated(CommentCreated $event): void
    {
    }
}
