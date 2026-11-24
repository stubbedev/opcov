<?php
function defaults($make = static function () { return 'deferred'; }) { return [$make, fn () => 'compiled']; }
