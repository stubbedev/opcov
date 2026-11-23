<?php
function report(): void
{
    $status = \opcov\get_status();
    $base = __DIR__ . '/';
    echo 'off: ', $status['off_reason'] === null ? 'null' : str_replace($base, '', $status['off_reason']), "\n";
    foreach ($status['selected_files'] as $file) {
        echo 'selected: ', str_replace($base, '', $file), "\n";
    }
    foreach ($status['skipped_files'] as $file => $reason) {
        if (str_starts_with($file, $base)) {
            echo 'skipped: ', str_replace($base, '', $file), ': ', str_replace($base, '', $reason), "\n";
        }
    }
}
