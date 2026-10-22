// 보고·질문 본문에만 적용한다. 외부 주소·경로 이탈 판정은 각 본문의 resolver가 맡는다.
export const isVideoPath = (src: string): boolean => /\.(mp4|webm)(?:[?#]|$)/i.test(src);
