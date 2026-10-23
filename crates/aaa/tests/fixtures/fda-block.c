/* 시험 전용: 시스템 TCC DB open만 FIFO로 바꾸고 session host에는 주입을 물려주지 않는다. */
#include <errno.h>
#include <fcntl.h>
#include <spawn.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* worker 말고 어느 자식까지 주입됐는지, 정리 전에도 PID로 대조할 수 있게 남긴다. */
__attribute__((constructor)) static void loaded(void) {
    const char *gate = getenv("AAA_TEST_FDA_GATE");
    if (!gate) return;
    char marker[4096], pid[32];
    if (snprintf(marker, sizeof(marker), "%s/loaded", gate) >= (int)sizeof(marker)) _exit(91);
    int fd = open(marker, O_WRONLY | O_CREAT | O_APPEND, 0600);
    int count = snprintf(pid, sizeof(pid), "%d\n", getpid());
    if (fd < 0 || write(fd, pid, count) != count || close(fd)) _exit(92);
}

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

static int probe_spawn(pid_t *pid, const char *path, const posix_spawn_file_actions_t *actions,
                       const posix_spawnattr_t *attrs, char *const argv[], char *const envp[]) {
    /* worker 자신의 exec 환경은 보존한다. agent가 여는 실제 session host 경계에서만 뺀다. */
    if (!argv || !argv[0] || !argv[1] || strcmp(argv[1], "session-host") != 0)
        return posix_spawn(pid, path, actions, attrs, argv, envp);
    size_t count = 0;
    while (envp[count]) ++count;
    char **clean = calloc(count + 1, sizeof(*clean));
    if (!clean) return ENOMEM;
    size_t next = 0;
    for (size_t i = 0; i < count; ++i) {
        if (strncmp(envp[i], "DYLD_INSERT_LIBRARIES=", sizeof("DYLD_INSERT_LIBRARIES=") - 1) != 0 &&
            strncmp(envp[i], "AAA_TEST_FDA_GATE=", sizeof("AAA_TEST_FDA_GATE=") - 1) != 0)
            clean[next++] = envp[i];
    }
    int rc = posix_spawn(pid, path, actions, attrs, argv, clean);
    free(clean);
    return rc;
}

__attribute__((used, section("__DATA,__interpose")))
static const struct { const void *replacement; const void *original; } hooks[] = {
    { (const void *)probe_open, (const void *)open },
    { (const void *)probe_spawn, (const void *)posix_spawn }
};
