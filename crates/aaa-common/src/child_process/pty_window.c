// 격리 worker에서 실제 openpty 반환과 portable-pty의 CLOEXEC 설정 사이만 붙잡는다.
#define _GNU_SOURCE
#include <dlfcn.h>
#include <pty.h>
#include <stdatomic.h>

static _Atomic int armed;
static void (*window_hook)(int, int);

void aaa_pty_arm(void (*hook)(int, int)) {
    window_hook = hook;
    atomic_store(&armed, 1);
}

int openpty(int *master, int *slave, char *name,
            const struct termios *mode, const struct winsize *size) {
    int (*original)(int *, int *, char *, const struct termios *, const struct winsize *) =
        dlsym(RTLD_NEXT, "openpty");
    if (!original) return -1;
    int result = original(master, slave, name, mode, size);
    if (result == 0 && atomic_exchange(&armed, 0)) window_hook(*master, *slave);
    return result;
}
