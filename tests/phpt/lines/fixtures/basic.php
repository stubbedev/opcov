<?php
function classify(int $n): string
{
    if ($n > 10) {
        return 'big';
    }
    $label = $n < 0
        ? 'negative'
        : 'small';
    return $label;
}

function unused(): void
{
    echo 'never';
}

$result = classify(3);
$object = new ArrayObject(
    [1, 2]
);
