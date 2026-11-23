<?php

declare(strict_types=1);

spl_autoload_register(static function (string $class): void {
    $prefix = 'OpCov\\PHPUnit\\';
    if (!str_starts_with($class, $prefix)) {
        return;
    }
    $file = dirname(__DIR__, 4) . '/php/phpunit/src/' . str_replace('\\', '/', substr($class, strlen($prefix))) . '.php';
    if (is_file($file)) {
        require $file;
    }
});
