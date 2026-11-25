<?php

namespace App\Providers;

use App\Services\Clock;
use Illuminate\Support\ServiceProvider;

class AppServiceProvider extends ServiceProvider
{
    public function register(): void
    {
        $this->app->singleton('time', function () {
            return new Clock();
        });
    }
}
