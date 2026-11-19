#include <stdio.h>

int greatest_common_divisor(int a, int b) {
    while (b != 0) {
        int next = a % b;
        a = b;
        b = next;
    }
    return a;
}
