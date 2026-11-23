<?php
$pair = [fn () => 1, fn () => 2];
$first = function () { return 3; }; $second = function () { return 4; };
$nested = fn () => fn () => 5;
$mixed = [function () { return 6; }, fn () => 7];
$classes = [new class { public function run() { return 8; } }, new class { public function run() { return 9; } }];
$outer = new class { public function make() { return new class { public function run() { return 10; } }; } };
function holder() { return [fn () => 11, fn () => 12]; }
