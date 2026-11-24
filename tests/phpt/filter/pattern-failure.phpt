--TEST--
An exclude pattern that fails on one file's path (a /u pattern meets a file name that is not UTF-8) skips only that file, with the reason and its code; collection stays on for every other file
--SKIPIF--
<?php
require __DIR__ . '/../support/coverage.inc';
if (!opcov_filesystem_takes_non_utf8_names()) {
    die('skip the file system refuses file names that are not UTF-8');
}
?>
--FILE--
<?php
require __DIR__ . '/../support/coverage.inc';
$dir = sys_get_temp_dir() . '/opcov-pattern-' . getmypid() . '-' . bin2hex(random_bytes(4));
mkdir($dir);
$latin = "$dir/caf\xE9.php";
file_put_contents($latin, "<?php\nfunction latin(): int\n{\n    return 1;\n}\n");
file_put_contents("$dir/plain.php", "<?php\nfunction plain(): int\n{\n    return 2;\n}\n");
$script = 'require ' . var_export($latin, true) . '; require ' . var_export("$dir/plain.php", true) . ";\n" . <<<'PHP'
\opcov\start();
latin();
plain();
$hits = \opcov\stop('T::t');
$status = \opcov\get_status();
$strip = static fn (string $text): string => addcslashes(str_replace(dirname(array_key_first($hits)) . '/', '', $text), "\x80..\xFF");
var_dump($status['off_reason']);
echo 'hits: ', implode(', ', array_map($strip, array_keys($hits))), "\n";
foreach ($status['skipped_files'] as $file => $reason) {
    if (str_contains($file, 'caf')) {
        echo 'skipped: ', $strip($file), ': ', preg_replace('~ \(see https://\S+#([a-z-]+)\)~', ' [$1]', $reason), "\n";
    }
}
\opcov\ensure_can_collect();
echo "can collect\n";

PHP;
echo opcov_child("<?php $script", [
    'opcov.enabled' => '1',
    'opcov.collect_paths' => $dir,
    'opcov.exclude_pattern' => '~nothing~u',
]);
unlink($latin);
unlink("$dir/plain.php");
rmdir($dir);
--EXPECT--
NULL
hits: plain.php
skipped: caf%E9.php: opcov.exclude_pattern failed to match against its resolved path [exclude-pattern-failed]
can collect
