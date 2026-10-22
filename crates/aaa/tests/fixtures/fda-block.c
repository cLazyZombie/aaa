/* 시험 전용: 시스템 TCC DB open만 FIFO로 바꾼다. 실제 TCC/설정은 건드리지 않는다. */
#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int probe_open(const char *path, int flags, ...) {
    mode_t mode = 0;
    if (flags & O_CREAT) {
        va_list ap; va_start(ap, flags); mode = va_arg(ap, int); va_end(ap);
    }
    const char *gate = getenv("AAA_TEST_FDA_GATE");
    if (gate && strcmp(path, "/Library/Application Support/com.apple.TCC/TCC.db") == 0) {
        char marker[4096], fifo[4096], pid[32];
        if (snprintf(marker, sizeof(marker), "%s/entered", gate) >= (int)sizeof(marker) ||
            snprintf(fifo, sizeof(fifo), "%s/open", gate) >= (int)sizeof(fifo)) _exit(91);
        int fd = open(marker, O_WRONLY | O_CREAT | O_APPEND, 0600);
        int count = snprintf(pid, sizeof(pid), "%d\n", getpid());
        if (fd < 0 || write(fd, pid, count) != count || close(fd)) _exit(92);
        return open(fifo, O_RDONLY); /* writer가 열릴 때까지 결정적으로 막힌다. */
    }
    return open(path, flags, mode);
}
__attribute__((used, section("__DATA,__interpose")))
static const struct { const void *replacement; const void *original; } hooks[] = {
    { (const void *)probe_open, (const void *)open }
};
