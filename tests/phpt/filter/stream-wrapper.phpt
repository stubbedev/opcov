--TEST--
Files loaded through a stream wrapper are not selectable by path and are skipped without error
--INI--
opcov.enabled=1
opcov.collect_paths={PWD}/fixtures
--FILE--
<?php
final class MemoryStream
{
    public $context;
    private int $position = 0;
    private string $code = "<?php return 40 + 2;";

    public function stream_open(string $path, string $mode, int $options, ?string &$opened): bool { return true; }
    public function stream_read(int $count): string { $chunk = substr($this->code, $this->position, $count); $this->position += strlen($chunk); return $chunk; }
    public function stream_eof(): bool { return $this->position >= strlen($this->code); }
    public function stream_stat(): array { return []; }
    public function stream_set_option(int $option, int $arg1, ?int $arg2): bool { return false; }
}
stream_wrapper_register("mem", MemoryStream::class);
var_dump(include "mem://answer.php");
echo \opcov\get_status()["skipped_files"]["mem://answer.php"], "\n";
--EXPECT--
int(42)
loaded through a stream wrapper, which is not selectable by path
