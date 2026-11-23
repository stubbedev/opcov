<?php
function piped(string $text): string
{
    return $text
        |> trim(...)
        |> (static fn (string $s): string => $s === '' ? 'empty' : strtoupper($s));
}
