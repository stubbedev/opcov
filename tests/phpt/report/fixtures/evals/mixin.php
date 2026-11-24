<?php
function mixin(string $code): mixed
{
    return eval($code);
}
