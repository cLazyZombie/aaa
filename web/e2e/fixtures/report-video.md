# 영상 시험 자료

`report-video.mp4`는 외부 자료가 없는 FFmpeg testsrc2 패턴이다. 320×180, 12 fps, 2초,
H.264 High, yuv420p, 음원 없음, faststart다. WebKit·Chromium에서 실제 decode·재생·탐색을 검사한다.

생성 명령:

```sh
ffmpeg -f lavfi -i testsrc2=size=320x180:rate=12 -t 2 -c:v libx264 -profile:v high -pix_fmt yuv420p -an -movflags +faststart report-video.mp4
```
