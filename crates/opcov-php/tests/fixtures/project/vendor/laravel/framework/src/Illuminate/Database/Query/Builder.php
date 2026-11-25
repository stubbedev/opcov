<?php

namespace Illuminate\Database\Query;

class Builder
{
    /** @return $this */
    public function orderBy(string $column)
    {
        return $this;
    }
}
