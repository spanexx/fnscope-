<?php

/**
 * Add two integers, second one optional.
 */
function add(int $a, int $b = 0): int
{
    return $a + $b;
}

class Calculator
{
    public int $factor = 2;

    public const SCALE = 3;

    /**
     * Scale a value by the configured factor.
     */
    public function scale(int $value): int
    {
        return add($value * $this->factor);
    }
}
