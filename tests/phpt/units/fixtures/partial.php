<?php
function join_words(string $first, string $second, string $third): string
{
    return "$first $second $third";
}

$greet = join_words('good', ?, 'world');
echo $greet('morning'), "\n";
