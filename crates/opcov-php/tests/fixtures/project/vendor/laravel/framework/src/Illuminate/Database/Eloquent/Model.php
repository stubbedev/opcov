<?php

namespace Illuminate\Database\Eloquent;

abstract class Model
{
    /** @return Builder<static> */
    public static function query()
    {
        return (new static())->newQuery();
    }

    /** @return Builder<static> */
    public function newQuery()
    {
        return $this->newEloquentBuilder(null);
    }

    /** @return Builder<static> */
    public function newEloquentBuilder($query)
    {
        return new Builder();
    }

    public function save(): bool
    {
        return true;
    }

    public function delete(): bool
    {
        return true;
    }

    /**
     * @template TRelated of Model
     * @param class-string<TRelated> $related
     * @return Relations\HasMany<TRelated, $this>
     */
    public function hasMany(string $related)
    {
        return new Relations\HasMany();
    }

    public static function __callStatic(string $method, array $arguments): mixed
    {
        return (new static())->$method(...$arguments);
    }

    public function __call(string $method, array $arguments): mixed
    {
        return $this->newQuery()->$method(...$arguments);
    }
}
