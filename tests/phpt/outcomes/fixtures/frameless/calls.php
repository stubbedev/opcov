<?php
namespace N;
function f(?int $x): int { return max(1, $x ?? 2); }
function g(string $s): bool { $r = str_contains($s,
 'b');
 return $r; }
function k(?int $x): int { return max(1, $x ?? 2); }
