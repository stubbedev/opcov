<?php
function opcov_lines(string $file, callable $run): string
{
    \opcov\start();
    $run();
    $hits = \opcov\stop();
    \opcov\reset();
    $covered = array_keys($hits[$file]['lines'] ?? []);
    $executable = array_keys(\opcov\executable([$file])[$file]['lines'] ?? []);
    $uncovered = array_values(array_diff($executable, $covered));
    return basename($file) . "\n  covered: " . implode(',', $covered) . "\n  uncovered: " . implode(',', $uncovered) . "\n";
}

function opcov_child(string $script, array $ini = [], ?string $binary = null, array $environment = [], string $options = ''): string
{
    $file = tempnam(sys_get_temp_dir(), 'opcov-child-');
    file_put_contents($file, $script);
    $settings = '';
    foreach ($ini as $name => $value) {
        $settings .= ' -d ' . escapeshellarg($name . '=' . $value);
    }
    $variables = '';
    foreach ($environment as $name => $value) {
        $variables .= ' ' . escapeshellarg($name . '=' . $value);
    }
    $executable = $binary === null ? getenv('TEST_PHP_EXECUTABLE_ESCAPED') : escapeshellarg($binary);
    $cgiRequest = ' -u REDIRECT_STATUS -u SCRIPT_FILENAME -u PATH_TRANSLATED -u REQUEST_METHOD -u QUERY_STRING -u CONTENT_TYPE -u CONTENT_LENGTH -u HTTP_COOKIE';
    $command = 'env' . $cgiRequest . $variables . ' ' . $executable . ' -n ' . $options . ' ' . getenv('TEST_PHP_EXTRA_ARGS') . $settings . ' ' . escapeshellarg($file) . ' 2>&1';
    $output = shell_exec($command);
    unlink($file);
    return (string) $output;
}

function opcov_filesystem_takes_non_utf8_names(): bool
{
    $directory = sys_get_temp_dir() . '/opcov-bytes-' . getmypid() . '-' . bin2hex(random_bytes(4));
    if (!@mkdir($directory)) {
        return false;
    }
    $file = $directory . "/caf\xE9.php";
    $taken = @file_put_contents($file, '') !== false;
    if ($taken) {
        unlink($file);
    }
    rmdir($directory);
    return $taken;
}

function opcov_sibling_binary(string $name): ?string
{
    $binary = dirname(PHP_BINARY) . '/' . $name;
    return is_executable($binary) ? $binary : null;
}

function opcov_opcache_ini(): ?array
{
    if (extension_loaded('Zend OPcache')) {
        return [];
    }
    $extension = getenv('OPCOV_OPCACHE_EXTENSION');
    return $extension === false || $extension === '' ? null : ['zend_extension' => $extension];
}

function opcov_jit_built(array $opcache): bool
{
    return opcov_child('<?php echo ini_get("opcache.jit") === false ? "no" : "yes";', $opcache) === 'yes';
}

function opcov_refusal(): string
{
    try {
        \opcov\ensure_can_collect();
        return "can collect\n";
    } catch (opcov\CannotCollectException $e) {
        return preg_replace('~ \(see https://\S+#([a-z-]+)\)~', ' [$1]', $e->getMessage()) . "\n";
    }
}

function opcov_kind(array $outcome): string
{
    $kind = $outcome['kind'];
    return is_array($kind) ? key($kind) . '(' . current($kind) . ')' : $kind;
}

function opcov_short_key(string $key): string
{
    return preg_replace('~@.*/~', '@', $key);
}

function opcov_branches(string $file, callable $run): string
{
    \opcov\start();
    try {
        $run();
    } catch (\Throwable $e) {
        echo 'threw ', get_class($e), "\n";
    }
    $hits = \opcov\stop();
    \opcov\reset();
    $model = \opcov\executable([$file])[$file]['functions'] ?? [];
    $text = '';
    foreach ($hits[$file]['functions'] ?? [] as $key => $function) {
        $taken = [];
        foreach ($model[$key]['outcomes'] as $outcome) {
            if (isset($function['outcomes'][$outcome['id']])) {
                $taken[] = opcov_kind($outcome) . '@' . $outcome['line'];
            }
        }
        foreach ($model[$key]['loops'] as $loop) {
            foreach (['zero', 'second'] as $flag) {
                if (isset($function['loops'][$loop['id']][$flag])) {
                    $taken[] = $loop['kind'] . '@' . $loop['line'] . ':' . $flag;
                }
            }
        }
        $text .= '  ' . opcov_short_key($key) . ': ' . implode(' ', $taken) . "\n";
    }
    return $text === '' ? "  (none)\n" : $text;
}
