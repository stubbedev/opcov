<?php
trait Greets
{
    public function greet(): string
    {
        return 'hello ' . $this->name();
    }
}

final class Ann
{
    use Greets;

    private function name(): string
    {
        return 'Ann';
    }
}

final class Bob
{
    use Greets;

    private function name(): string
    {
        return 'Bob';
    }
}
