<?php

declare(strict_types=1);

return [
    static function (): void {
        $customers = [null, (object) ['note' => 'vip'], (object) ['note' => null]];
        $countries = ['DK', 'NO', 'DE', 'FR', 'XX'];
        $checksum = 0;
        for ($i = 0; $i < 400_000; $i++) {
            $order = [
                'total' => $i % 1500,
                'country' => $countries[$i % 5],
                'priority' => $i % 7 === 0 ? 'high' : '',
                'customer' => $customers[$i % 3],
                'items' => [
                    ['qty' => $i % 3, 'price' => $i % 700],
                    ['qty' => 1, 'price' => $i % 90],
                ],
            ];
            $checksum += strlen(classify($order));
        }
        if ($checksum === 0) {
            throw new LogicException('the workload computed nothing');
        }
    },
];
