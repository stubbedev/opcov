<?php
function zeta() { return fn () => 1; } function alpha() { return fn () => 2; }
class Beta { public function m() { return fn () => 3; } } class Alpha { public function m() { return fn () => 4; } }
function main_peer() { return fn () => 5; } $main = fn () => 6;
function host() { function guest() { return fn () => 7; } return fn () => 8; } $hosted = fn () => 9;
if (true) { function conditional() { return fn () => 10; } } $branch = fn () => 11;
{ function blocky() { return fn () => 12; } } $block = fn () => 13;
$anonymous = new class { public function m() { return fn () => 14; } }; $plain = fn () => 15;
class Hooked { public int $p { get => (fn () => 16)(); } } $hook = fn () => 17;
$destructure = function () { [$d[(fn () => 18)()]] = [fn () => 19]; return $d; };
$nested = fn () => [fn () => 20, function () { return fn () => 21; }];
