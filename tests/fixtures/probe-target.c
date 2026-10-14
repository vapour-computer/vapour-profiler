// SPDX-License-Identifier: MIT OR Apache-2.0
#define _POSIX_C_SOURCE 200809L
#include <time.h>

__attribute__((noinline)) void vp_test_frame(void)
{
    __asm__ volatile("" ::: "memory");
}

int main(void)
{
    const struct timespec interval = { .tv_sec = 0, .tv_nsec = 10000000 };
    for (;;) {
        vp_test_frame();
        nanosleep(&interval, 0);
    }
}
