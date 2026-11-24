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

final class OpcovProcess
{
    public function __construct(public readonly string $output, public readonly int $status)
    {
    }
}

function opcov_shell_words(string $text): array
{
    $words = [];
    $word = null;
    $length = strlen($text);
    for ($at = 0; $at < $length; $at++) {
        $char = $text[$at];
        if ($char === ' ' || $char === "\t" || $char === "\n") {
            if ($word !== null) {
                $words[] = $word;
                $word = null;
            }
            continue;
        }
        $word ??= '';
        if ($char === "'") {
            $close = strpos($text, "'", $at + 1);
            if ($close === false) {
                throw new UnexpectedValueException("unterminated single quote at byte $at of shell words: $text");
            }
            $word .= substr($text, $at + 1, $close - $at - 1);
            $at = $close;
            continue;
        }
        if ($char === '"') {
            for ($at++; $at < $length && $text[$at] !== '"'; $at++) {
                if ($text[$at] === '$' || $text[$at] === '`') {
                    throw new UnexpectedValueException("shell expansion at byte $at of shell words: $text");
                }
                if ($text[$at] === '\\' && $at + 1 < $length && str_contains("\$`\"\\\n", $text[$at + 1])) {
                    $at++;
                }
                $word .= $text[$at];
            }
            if ($at >= $length) {
                throw new UnexpectedValueException("unterminated double quote in shell words: $text");
            }
            continue;
        }
        if ($char === '\\') {
            if (++$at >= $length) {
                throw new UnexpectedValueException("trailing backslash in shell words: $text");
            }
            $word .= $text[$at];
            continue;
        }
        if (str_contains('$`;|&<>()*?[]{}~#', $char)) {
            throw new UnexpectedValueException("unquoted shell metacharacter '$char' at byte $at of shell words: $text");
        }
        $word .= $char;
    }
    if ($word !== null) {
        $words[] = $word;
    }
    return $words;
}

function opcov_extra_args(): array
{
    static $arguments = null;
    return $arguments ??= opcov_shell_words((string) getenv('TEST_PHP_EXTRA_ARGS'));
}

function opcov_spawn(array $command, array $environment = []): OpcovProcess
{
    $inherited = getenv();
    foreach (['REDIRECT_STATUS', 'SCRIPT_FILENAME', 'PATH_TRANSLATED', 'REQUEST_METHOD', 'QUERY_STRING', 'CONTENT_TYPE', 'CONTENT_LENGTH', 'HTTP_COOKIE'] as $cgiRequest) {
        unset($inherited[$cgiRequest]);
    }
    $process = proc_open($command, [0 => ['null'], 1 => ['pipe', 'w'], 2 => ['redirect', 1]], $pipes, null, $environment + $inherited);
    if ($process === false) {
        throw new RuntimeException('cannot start ' . implode(' ', $command));
    }
    $output = stream_get_contents($pipes[1]);
    fclose($pipes[1]);
    $status = proc_close($process);
    if ($output === false) {
        throw new RuntimeException('cannot read the output of ' . implode(' ', $command));
    }
    return new OpcovProcess($output, $status);
}

function opcov_cli(array $arguments, array $environment = []): OpcovProcess
{
    return opcov_spawn([getenv('OPCOV_CLI') ?: throw new RuntimeException('OPCOV_CLI does not name the opcov binary'), ...$arguments], $environment);
}

function opcov_child(string $script, array $ini = [], ?string $binary = null, array $environment = [], array $options = []): string
{
    $file = tempnam(sys_get_temp_dir(), 'opcov-child-');
    file_put_contents($file, $script);
    $command = [$binary ?? (getenv('TEST_PHP_EXECUTABLE') ?: throw new RuntimeException('TEST_PHP_EXECUTABLE does not name the PHP under test')), '-n', ...$options, ...opcov_extra_args()];
    foreach ($ini as $name => $value) {
        array_push($command, '-d', $name . '=' . $value);
    }
    $command[] = $file;
    try {
        return opcov_spawn($command, $environment)->output;
    } finally {
        unlink($file);
    }
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

function opcov_jit_switchable(array $opcache): bool
{
    $settings = ['opcache.enable_cli' => '1', 'opcache.jit' => 'off', 'opcache.jit_buffer_size' => '16M'] + $opcache;
    return opcov_child('<?php echo @ini_set("opcache.jit", "tracing") === false ? "no" : "yes";', $settings) === 'yes';
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
