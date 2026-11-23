<?php

declare(strict_types=1);

final class Ledger
{
    /** @var array<string, int> */
    private array $balances = [];

    /** @var list<array{string, string, int}> */
    private array $journal = [];

    public function open(string $account, int $initial = 0): void
    {
        if (isset($this->balances[$account])) {
            throw new DomainException("account $account exists");
        }
        if ($initial < 0) {
            throw new DomainException('negative opening balance');
        }
        $this->balances[$account] = $initial;
    }

    public function transfer(string $from, string $to, int $amount): void
    {
        if ($amount <= 0) {
            throw new DomainException('amount must be positive');
        }
        $available = $this->balances[$from] ?? throw new DomainException("unknown account $from");
        if (!isset($this->balances[$to])) {
            throw new DomainException("unknown account $to");
        }
        if ($available < $amount) {
            throw new DomainException('insufficient funds');
        }
        $this->balances[$from] = $available - $amount;
        $this->balances[$to] += $amount;
        $this->journal[] = [$from, $to, $amount];
    }

    public function balance(string $account): int
    {
        return $this->balances[$account] ?? throw new DomainException("unknown account $account");
    }

    public function statement(string $account): array
    {
        $lines = [];
        foreach ($this->journal as [$from, $to, $amount]) {
            if ($from === $account) {
                $lines[] = -$amount;
            } elseif ($to === $account) {
                $lines[] = $amount;
            }
        }
        return $lines;
    }

    public function audit(): bool
    {
        $sum = 0;
        foreach ($this->balances as $balance) {
            if ($balance < 0) {
                return false;
            }
            $sum += $balance;
        }
        return $sum >= 0;
    }

    public function close(string $account): int
    {
        $balance = $this->balance($account);
        if ($balance !== 0) {
            throw new DomainException('account is not empty');
        }
        unset($this->balances[$account]);
        return count($this->balances);
    }
}
