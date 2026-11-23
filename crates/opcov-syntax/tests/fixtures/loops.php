<?php
function loops(array $items, $n)
{
    for ($i = 0; $i < $n; $i++) {
        echo $i;
    }
    for (;;) {
        break;
    }
    while ($n > 0) {
        $n--;
    }
    do {
        $n++;
    } while ($n < 3);
    foreach ($items as $key => $item) {
        echo $key, $item;
    }
    try {
        return $n;
    } catch (\LogicException | \RuntimeException $e) {
        echo 1;
    } catch (\Exception) {
        echo 2;
    } finally {
        echo 3;
    }
}
