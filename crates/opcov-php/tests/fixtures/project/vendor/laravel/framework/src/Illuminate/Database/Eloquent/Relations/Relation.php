<?php

namespace Illuminate\Database\Eloquent\Relations;

/**
 * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
 * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
 * @mixin \Illuminate\Database\Eloquent\Builder<TRelatedModel>
 */
abstract class Relation
{
    public function __call(string $method, array $arguments): mixed
    {
        return $this;
    }
}
