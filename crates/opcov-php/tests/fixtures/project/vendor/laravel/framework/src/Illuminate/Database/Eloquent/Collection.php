<?php

namespace Illuminate\Database\Eloquent;

/**
 * @template TKey of array-key
 * @template TModel
 * @implements \IteratorAggregate<TKey, TModel>
 */
class Collection implements \IteratorAggregate
{
    /** @return TModel|null */
    public function first()
    {
        return null;
    }

    public function getIterator(): \ArrayIterator
    {
        return new \ArrayIterator([]);
    }
}
