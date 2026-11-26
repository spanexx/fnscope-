"""Arithmetic helpers with one class method that calls a free function."""
from __future__ import annotations


def add(a: int, b: int) -> int:
    return a + b


class Calculator:
    def __init__(self, factor: int):
        self.factor = factor

    def scale(self, value: int) -> int:
        # Multiply through the helper.
        scaled = add(value, self.factor)
        return scaled
