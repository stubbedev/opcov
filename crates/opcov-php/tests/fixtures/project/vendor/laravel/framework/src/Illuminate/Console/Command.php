<?php

namespace Illuminate\Console;

abstract class Command
{
    public function call(string $command, array $arguments = []): int
    {
        return 0;
    }
}
