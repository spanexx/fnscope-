<?php

/**
 * Trim and lower-case a label.
 */
function normalize_text(string $text): string
{
    // Normalize whitespace first.
    $trimmed = trim($text);
    return strtolower($trimmed);
}

/**
 * Classify a label, rejecting empty input.
 */
function classify(string $text): string
{
    // Normalize before the guard.
    $cleaned = normalize_text($text);
    if ($cleaned === '') {
        throw new FunctionError("EMPTY_TEXT");
    }
    return $cleaned;
}

/**
 * Print a readable report.
 */
function report(string $text): void
{
    echo "report: " . $text;
    printf("done\n");
}
