<?php
function join_words(string $first, string $second): string { return "$first $second"; }
$hello = join_words('hello', ?); $loud = fn (string $text) => strtoupper($text);
