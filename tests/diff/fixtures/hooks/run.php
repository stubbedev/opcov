<?php

declare(strict_types=1);

return [
    static function (): void {
        $temperature = new Temperature();
        $temperature->fahrenheit = 50;
        echo $temperature->label, $temperature->celsius;
    },
    static function (): void {
        $temperature = new Temperature();
        try {
            $temperature->celsius = -300;
        } catch (InvalidArgumentException) {
        }
    },
];
