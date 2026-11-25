<?php

namespace App\Services;

interface ProvidesReport
{
    public function render(): string;
}

class Report implements ProvidesReport
{
    public const LIMIT = 10;

    public string $title = '';

    public function __construct()
    {
    }

    public function render(): string
    {
        return '';
    }
}

class InheritingReport extends Report
{
}

class OverridingReport extends Report
{
    public function render(): string
    {
        return 'own';
    }
}

class Unrelated
{
    public function render(): string
    {
        return '';
    }
}
