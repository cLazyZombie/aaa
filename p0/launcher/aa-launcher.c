/*
 * aa-launcher: 한 번 만들고 다시 빌드하지 않는 실행 파일(결정 14).
 *
 * 시스템 설정의 개발자 도구·전체 디스크 접근 목록에는 이 launcher만 등록한다. LaunchAgent나 LaunchServices가
 * launcher를 띄우면 launcher가 그때의 aa 실행 파일을 자식으로 띄운다. aa는 자유롭게 다시 빌드·교체한다.
 * launcher 자신은 바뀌지 않으므로 adhoc 서명이어도 cdhash가 그대로이고 등록이 유지된다는 전제다.
 *
 * 사용: aa-launcher <aa 실행 파일 절대 경로> [인자...]
 *   - 자식을 posix_spawn으로 띄우고(exec 하지 않는다) 끝날 때까지 기다린다.
 *   - SIGTERM·SIGINT·SIGHUP·SIGQUIT·SIGUSR1·SIGUSR2는 자식에게 전달한다.
 *   - 자식의 종료 코드를 그대로 돌려준다(신호로 끝나면 128+신호).
 *   - 이 파일은 일부러 기능을 더하지 않는다. 바꾸면 등록을 다시 해야 한다.
 */
#include <errno.h>
#include <signal.h>
#include <spawn.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

extern char **environ;

static volatile sig_atomic_t child = 0;

static void forward(int sig) {
    if (child > 0) kill((pid_t)child, sig);
}

int main(int argc, char **argv) {
    if (argc < 2 || argv[1][0] != '/') {
        fprintf(stderr, "aa-launcher: 사용법: aa-launcher <aa 실행 파일 절대 경로> [인자...]\n");
        return 64;
    }
    static const int sigs[] = {SIGTERM, SIGINT, SIGHUP, SIGQUIT, SIGUSR1, SIGUSR2};
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = forward;
    sigemptyset(&sa.sa_mask);
    for (size_t i = 0; i < sizeof sigs / sizeof sigs[0]; i++) sigaction(sigs[i], &sa, NULL);

    pid_t pid;
    int rc = posix_spawn(&pid, argv[1], NULL, NULL, &argv[1], environ);
    if (rc != 0) {
        fprintf(stderr, "aa-launcher: %s 실행 실패: %s\n", argv[1], strerror(rc));
        return 127;
    }
    child = pid;
    int status;
    for (;;) {
        if (waitpid(pid, &status, 0) == pid) break;
        if (errno != EINTR) {
            perror("aa-launcher: waitpid");
            return 70;
        }
    }
    if (WIFEXITED(status)) return WEXITSTATUS(status);
    if (WIFSIGNALED(status)) return 128 + WTERMSIG(status);
    return 70;
}
