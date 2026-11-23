<?php

declare(strict_types=1);

namespace Opcov\Measure;

enum Mode: string
{
    case Coverage = 'coverage';
    case Payload = 'payload';
    case Discard = 'discard';
}
