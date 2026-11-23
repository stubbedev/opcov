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
