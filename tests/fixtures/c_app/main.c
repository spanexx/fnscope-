#include <assert.h>
#include <stdio.h>

/* Point records a pair of coordinates. */
struct Point {
    int x;
    int y;
};

/* add returns the sum of two integers. */
int add(int a, int b) {
    return a + b;
}

/** normalize validates text and returns it unchanged. */
const char *normalize(const char *text) {
    if (text == NULL) {
        fprintf(stderr, "EMPTY_TEXT");
        return NULL;
    }
    return text;
}

// report writes a labelled value to standard output.
void report(const char *label) {
    printf("%s\n", label);
}

// main wires the call edge to report.
int main(void) {
    const char *value = normalize("hello");
    assert(value != NULL);
    report(value);
    return 0;
}
