// 실험실 자식에서만 Rust std의 실제 pipe() 반환 직전 창을 사건으로 붙잡는다.
// 원래 pipe를 호출하며 FD·CLOEXEC·spawn 동작은 바꾸지 않는다.
#include <stdatomic.h>
#include <unistd.h>

static _Atomic int armed;
static void (*window_hook)(int, int);

void aaa_pipe_arm(void (*hook)(int, int)) {
    window_hook = hook;
    atomic_store(&armed, 1);
}

static int intercepted_pipe(int fds[2]) {
    int result = pipe(fds);
    if (result == 0 && atomic_exchange(&armed, 0)) {
        window_hook(fds[0], fds[1]);
    }
    return result;
}

__attribute__((used, section("__DATA,__interpose")))
static struct { const void *replacement; const void *original; } interpose_pipe = {
    (const void *)intercepted_pipe, (const void *)pipe
};
