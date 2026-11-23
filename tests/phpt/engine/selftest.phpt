--TEST--
Every engine symbol the probes need is reachable and behaves as the probe-label trick requires
--FILE--
<?php
foreach ([1, 2] as $value) {
    if ($value) {
        echo '';
    }
}
echo \opcov\engine_selftest(), "\n";
--EXPECTF--
vm kind: %r(CALL|HYBRID|TAILCALL)%r
probe label: ok
compile hooks: ok
engine globals: ok
resource handle: ok
extension registration: ok
caller cfg: %d blocks, loop headers: 1
