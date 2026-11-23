<?php
function opcov_finish_interrupted(string $file, string $test): void
{
    $status = \opcov\get_status();
    echo "collecting at shutdown: ", json_encode($status['collecting']), "\n";
    \opcov\stop($test);
    $report = tempnam(sys_get_temp_dir(), 'opcov-report-');
    \opcov\write_report($report, ['absolute_paths' => true]);
    $written = json_decode(file_get_contents($report), true);
    unlink($report);
    $lines = array_keys(array_filter($written['files'][$file]['lines'], static fn (array $tests): bool => $tests !== []));
    echo "tests: ", json_encode($written['tests']), "\n";
    echo "lines: ", implode(',', $lines), "\n";
}
