<?php

declare(strict_types=1);

namespace Opcov\Measure\Parity;

enum Aspect: string
{
    case Lines = 'lines';
    case Outcomes = 'outcomes';
    case Expected = 'expected';
}
