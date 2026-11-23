<?php

declare(strict_types=1);

namespace Opcov\Measure;

use PHPUnit\Event\Test\Finished;
use PHPUnit\Event\Test\FinishedSubscriber;
use PHPUnit\Event\Test\PreparationStarted;
use PHPUnit\Event\Test\PreparationStartedSubscriber;
use PHPUnit\Event\TestRunner\ExecutionFinished;
use PHPUnit\Event\TestRunner\ExecutionFinishedSubscriber;
use PHPUnit\Runner\Extension\Extension;
use PHPUnit\Runner\Extension\Facade;
use PHPUnit\Runner\Extension\ParameterCollection;
use PHPUnit\TextUI\Configuration\Configuration;

final class PhpUnitExtension implements Extension
{
    private ?Run $run = null;

    public function bootstrap(Configuration $configuration, Facade $facade, ParameterCollection $parameters): void
    {
        $this->run = new Run(Config::fromEnvironment());
        $extension = $this;
        $facade->registerSubscribers(
            new class ($extension) implements PreparationStartedSubscriber {
                public function __construct(private readonly PhpUnitExtension $extension)
                {
                }

                public function notify(PreparationStarted $event): void
                {
                    $this->extension->run()->beginTest($event->test()->id());
                }
            },
            new class ($extension) implements FinishedSubscriber {
                public function __construct(private readonly PhpUnitExtension $extension)
                {
                }

                public function notify(Finished $event): void
                {
                    $this->extension->run()->endTest();
                }
            },
            new class ($extension) implements ExecutionFinishedSubscriber {
                public function __construct(private readonly PhpUnitExtension $extension)
                {
                }

                public function notify(ExecutionFinished $event): void
                {
                    $this->extension->run()->finish();
                }
            },
        );
    }

    public function run(): Run
    {
        return $this->run ?? throw new \LogicException('a test event arrived before the extension was bootstrapped');
    }
}
