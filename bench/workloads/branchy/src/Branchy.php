<?php

declare(strict_types=1);

function classify(array $order): string
{
    $total = $order['total'] ?? 0;
    $country = $order['country'] ?? 'XX';
    $priority = $order['priority'] ?: 'normal';
    $flags = 0;
    if ($total > 1000 && $country !== 'XX') {
        $flags |= 1;
    } elseif ($total > 100 || $priority === 'high') {
        $flags |= 2;
    } else {
        $flags |= 4;
    }
    foreach ($order['items'] as $item) {
        if ($item['qty'] <= 0) {
            continue;
        }
        $flags |= match (true) {
            $item['price'] > 500 => 8,
            $item['price'] > 50 => 16,
            default => 32,
        };
    }
    switch ($country) {
        case 'DK':
        case 'NO':
            $region = 'nordic';
            break;
        case 'DE':
            $region = 'central';
            break;
        default:
            $region = 'other';
    }
    $note = $order['customer']?->note ?? '';
    return $region . ':' . $flags . ($note === '' ? '' : '!');
}
