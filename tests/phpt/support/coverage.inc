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

function opcov_child(string $script, array $ini = []): string
{
    $file = tempnam(sys_get_temp_dir(), 'opcov-child-');
    file_put_contents($file, $script);
    $settings = '';
    foreach ($ini as $name => $value) {
        $settings .= ' -d ' . escapeshellarg($name . '=' . $value);
    }
    $command = getenv('TEST_PHP_EXECUTABLE_ESCAPED') . ' -n ' . getenv('TEST_PHP_EXTRA_ARGS') . $settings . ' ' . escapeshellarg($file) . ' 2>&1';
    $output = shell_exec($command);
    unlink($file);
    return (string) $output;
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
