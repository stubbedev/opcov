<?php

namespace App\Uses;

use App\Models\Comment;
use App\Models\Post;

final class Eloquent
{
    public function run(Post $post, Comment $comment): void
    {
        Post::where('slug', 'a')->published()->first()->publish();
        Post::whereSlug('a')->first()->publish();
        Post::query()->orderBy('id')->published()->first()->publish();
        $post->comments()->visible()->first()->approve();
        $post->comments->first()->approve();
        Comment::visible()->first()->approve();
        $post->save();
        Post::query()->published()->create([]);
        $comment->save();
    }
}
