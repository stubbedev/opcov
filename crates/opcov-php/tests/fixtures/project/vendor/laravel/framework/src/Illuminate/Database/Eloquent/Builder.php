<?php

namespace Illuminate\Database\Eloquent;

/**
 * @template TModel of Model
 * @mixin \Illuminate\Database\Query\Builder
 */
class Builder
{
    /** @return $this */
    public function where(string $column, mixed $value = null)
    {
        return $this;
    }

    /** @return TModel|null */
    public function first()
    {
        return null;
    }

    /** @return TModel */
    public function create(array $attributes = [])
    {
        throw new \LogicException();
    }

    /** @return Collection<int, TModel> */
    public function get()
    {
        return new Collection();
    }

    public function __call(string $method, array $arguments): mixed
    {
        return $this;
    }
}
