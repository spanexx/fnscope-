#include <cstdio>
#include <iostream>
#include <stdexcept>

// Add two values.
int add(int a, int b) {
  int total = a + b;
  return total;
}

int normalize(int n) {
  int shifted = add(n, 1);
  return shifted;
}

// A small calculator.
class Calculator {
public:
  // Build with a scale factor.
  Calculator(int factor);
  // Scale one value.
  int scale(int n) { return n * factor_; }
private:
  int factor_;
};

Calculator::Calculator(int factor) : factor_(factor) {}

// Reject negative input.
int guard(int n) {
  if (n < 0) {
    throw std::runtime_error("NEGATIVE_INPUT");
  }
  return n;
}

// Report a value to the console.
void report(int n) {
  std::cout << n;
  printf("done\n");
}
