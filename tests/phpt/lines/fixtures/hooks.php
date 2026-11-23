<?php
final class Person
{
    public string $name {
        get => ucfirst($this->name);
        set (string $value) {
            $this->name = trim($value);
        }
    }
}

$person = new Person();
$person->name = ' ada ';
echo $person->name, "\n";
