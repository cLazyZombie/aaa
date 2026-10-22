import "./ListId.css";

/** 보고서·질문·할 일·자동화 목록이 공유하는 고정 번호 표지. 이동은 둘러싼 행이 맡는다. */
export function ListId({ id, testId }: { id: string; testId?: string }) {
  return <code className="list-id" data-testid={testId}>{id}</code>;
}
