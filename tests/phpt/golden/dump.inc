<?php
declare(strict_types=1);

const SNAPSHOT = <<<'PHP'
<?php
$fixture = json_decode(stream_get_contents(STDIN), true, flags: JSON_THROW_ON_ERROR);
$directory = getenv('OPCOV_DUMP_DIR');
$file = $directory . '/' . $fixture['file'];
file_put_contents($file, $fixture['source']);
include $file;
$strip = static function (array $value) use ($directory): array {
    $prefix = trim(json_encode($directory . '/', JSON_THROW_ON_ERROR), '"');
    return json_decode(str_replace($prefix, '', json_encode($value, JSON_THROW_ON_ERROR)), true, flags: JSON_THROW_ON_ERROR);
};
$key = $fixture['key'];
$expected = $strip(\opcov\get_debug_info($file)['functions'])[$key] ?? throw new RuntimeException("opcov measured no function $key in {$fixture['file']}");
$input = $strip(\opcov\selftest_cfg_inputs($file))[$key] ?? throw new RuntimeException("opcov kept no CFG input for $key in {$fixture['file']}");
unlink($file);
echo json_encode(['input' => $input, 'expected' => $expected], JSON_THROW_ON_ERROR);
PHP;

function fail(string $message): never
{
    fwrite(STDERR, "dump: $message\n");
    exit(1);
}

function minor(string $version): string
{
    return implode('.', array_slice(explode('.', $version), 0, 2));
}

function fixture_name(string $path): string
{
    return explode('.', basename($path), 2)[0];
}

function read_json(string $path): array
{
    $text = file_get_contents($path);
    if ($text === false) {
        fail("cannot read $path");
    }
    return json_decode($text, true, flags: JSON_THROW_ON_ERROR);
}

function write_json(string $path, array $value): void
{
    $text = json_encode($value, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR);
    $text = preg_replace_callback('/^( +)/m', static fn (array $match): string => str_repeat(' ', intdiv(strlen($match[1]), 2)), $text) . "\n";
    $temporary = $path . '.tmp';
    if (file_put_contents($temporary, $text) === false || !rename($temporary, $path)) {
        fail("cannot write $path");
    }
}

function snapshot(string $extension, string $directory, array $fixture): array
{
    $script = tempnam(sys_get_temp_dir(), 'opcov-dump-');
    file_put_contents($script, SNAPSHOT);
    $command = [PHP_BINARY, '-n', '-d', "extension=$extension", '-d', 'opcov.enabled=1', '-d', 'display_errors=stderr', '-d', "opcov.collect_paths=$directory", $script];
    $environment = ['OPCOV_DUMP_DIR' => $directory, 'PATH' => (string) getenv('PATH')];
    $process = proc_open($command, [0 => ['pipe', 'r'], 1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes, null, $environment);
    if ($process === false) {
        fail('cannot start ' . PHP_BINARY);
    }
    fwrite($pipes[0], json_encode($fixture, JSON_THROW_ON_ERROR));
    fclose($pipes[0]);
    $output = stream_get_contents($pipes[1]);
    $errors = stream_get_contents($pipes[2]);
    fclose($pipes[1]);
    fclose($pipes[2]);
    $status = proc_close($process);
    unlink($script);
    if ($status !== 0) {
        fail("snapshotting {$fixture['key']} in {$fixture['file']} failed with status $status: $output$errors");
    }
    return json_decode($output, true, flags: JSON_THROW_ON_ERROR);
}

function variants(string $directory, string $name): array
{
    $paths = glob("$directory/$name.json") ?: [];
    foreach (glob("$directory/$name.*.json") ?: [] as $path) {
        $paths[] = $path;
    }
    return $paths;
}

function record(string $golden, array $dump): void
{
    $directory = dirname($golden);
    $name = fixture_name($golden);
    $running = minor(PHP_VERSION);
    $files = [];
    foreach (variants($directory, $name) as $path) {
        $files[$path] = read_json($path);
    }
    $matching = null;
    foreach ($files as $path => $content) {
        $content['php'] = array_values(array_filter($content['php'], static fn (string $version): bool => minor($version) !== $running));
        $files[$path] = $content;
        $same = ($content['input'] ?? null) == $dump['input'] && ($content['expected'] ?? null) == $dump['expected'];
        if ($same && $matching === null) {
            $matching = $path;
        }
    }
    if ($matching === null) {
        foreach ($files as $path => $content) {
            if ($content['php'] === []) {
                $matching = $path;
                break;
            }
        }
    }
    if ($matching === null) {
        $matching = "$directory/$name." . str_replace('.', '', $running) . '.json';
        $template = reset($files);
        $files[$matching] = ['php' => [], 'source' => $template['source']];
    }
    $content = $files[$matching];
    $content['php'][] = PHP_VERSION;
    usort($content['php'], 'version_compare');
    $unchanged = ($content['input'] ?? null) == $dump['input'] && ($content['expected'] ?? null) == $dump['expected'];
    $snapshot = $unchanged ? $content : $dump;
    $files[$matching] = ['php' => $content['php'], 'source' => $content['source'], 'input' => $snapshot['input'], 'expected' => $snapshot['expected']];
    foreach ($files as $path => $content) {
        if ($content['php'] === []) {
            unlink($path);
            continue;
        }
        write_json($path, $content);
    }
}

[$self, $extension] = array_pad(array_slice($argv, 0, 2), 2, null);
$goldens = array_slice($argv, 2);
if ($extension === null || $goldens === []) {
    fail("usage: php $self <opcov extension built with engine-selftest> <golden.json>...");
}
$directory = sys_get_temp_dir() . '/opcov-dump-' . getmypid();
if (!mkdir($directory)) {
    fail("cannot create $directory");
}
$directory = realpath($directory);
$done = [];
foreach ($goldens as $golden) {
    $name = fixture_name($golden);
    if (isset($done[$name])) {
        continue;
    }
    $done[$name] = true;
    $template = read_json($golden);
    $fixture = [
        'source' => $template['source'],
        'key' => $template['input']['key'],
        'file' => $template['input']['file'],
    ];
    record($golden, snapshot($extension, $directory, $fixture));
    echo "$name: recorded for PHP ", PHP_VERSION, "\n";
}
rmdir($directory);
