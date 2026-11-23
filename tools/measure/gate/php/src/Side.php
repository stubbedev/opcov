<?php

declare(strict_types=1);

namespace Opcov\Measure\Gate;

enum Side: string
{
    case Both = 'both';
    case OpcovOnly = 'opcov only';
    case XdebugOnly = 'Xdebug only';
}
