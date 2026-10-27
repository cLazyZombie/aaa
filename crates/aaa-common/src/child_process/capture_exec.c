// 시스템 arm64e shell을 복사·재서명하지 않고 같은 아키텍처로 exec 경계를 검사한다.
// stdout/stderr는 fixture에 맡기고 단계·실제 로드 아키텍처만 부모의 trace에 남긴다.
#include <errno.h>
#include <mach-o/dyld.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    const char *path = getenv("AAA_PIPE_TRACE");
    FILE *trace = path ? fopen(path, "a") : NULL;
    if (!trace) {
        perror("capture exec trace");
        return 90;
    }
    const struct mach_header *header = _dyld_get_image_header(0);
    int injected = getenv("DYLD_INSERT_LIBRARIES") != NULL;
    fprintf(trace, "pid=%d exec launcher cputype=%d cpusubtype=%d injected=%d\n",
            getpid(), header->cputype, header->cpusubtype, injected);
    if (injected || argc < 2) {
        fprintf(trace, "pid=%d exec launcher invalid injected=%d argc=%d\n", getpid(), injected, argc);
        fclose(trace);
        return 91;
    }
    fprintf(trace, "pid=%d exec fixture\n", getpid());
    fclose(trace);
    // 판 조회 owner가 붙인 --version은 이전 sh -c의 $0처럼 fixture 인자로 전달하지 않는다.
    if (argc > 2 && strcmp(argv[argc - 1], "--version") == 0) {
        argv[argc - 1] = NULL;
    }
    execv(argv[1], argv + 1);
    int error = errno;
    fprintf(stderr, "capture exec failed errno=%d\n", error);
    return 92;
}
