<?php
function opcov_report_dir(): string
{
    $dir = sys_get_temp_dir() . '/opcov-report-' . getmypid() . '-' . bin2hex(random_bytes(4));
    mkdir($dir);
    return $dir;
}

function opcov_remove_dir(string $dir): void
{
    foreach (scandir($dir) as $entry) {
        if ($entry !== '.' && $entry !== '..') {
            unlink("$dir/$entry");
        }
    }
    rmdir($dir);
}

function opcov_masked(string $json, array $replace): string
{
    $report = json_decode($json, true, flags: JSON_THROW_ON_ERROR);
    $report['generator'] = array_map(static fn ($value) => gettype($value), $report['generator']);
    return strtr(json_encode($report, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES), $replace);
}

function opcov_run_tests(): void
{
    \opcov\start();
    add(1, 2);
    sign(1);
    \opcov\stop('Tests\CalcTest::test_add');
    \opcov\reset();
    \opcov\start();
    sign(-1);
    \opcov\stop('Tests\CalcTest::test_sign');
    \opcov\reset();
}
