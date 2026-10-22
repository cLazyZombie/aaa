import { useState } from "react";
import { drawerTrace, drawerTraceEnabled, drawerTraceText } from "./herdr/drawerTrace";
import "./DrawerDiagnostics.css";

/** 실기에서 몸짓 결과를 직접 표시하고 사건 기록을 꺼낸다. 켜진 탭의 메모리만 쓴다. */
export function DrawerDiagnostics() {
  const [text, setText] = useState("");
  const [normal, setNormal] = useState(0);
  const [preview, setPreview] = useState(0);
  const [staysOpen, setStaysOpen] = useState(0);
  if (!drawerTraceEnabled()) return null;
  const result = (kind: "normal" | "preview-only" | "stays-open") => {
    drawerTrace(`result:${kind}`);
    if (kind === "normal") setNormal((n) => n + 1);
    else if (kind === "preview-only") setPreview((n) => n + 1);
    else setStaysOpen((n) => n + 1);
    setText(drawerTraceText());
  };
  return <details className="drawer-diagnostics" data-testid="drawer-diagnostics">
    <summary>쓸기 진단</summary>
    <p>A → 메뉴에서 B 선택 → 가장자리 뒤로. 몸짓 뒤 이 창을 열어 결과를 한 번 누른다.</p>
    <p>정상 {normal}회 · 미리보기만 {preview}회 · 계속 열림 {staysOpen}회</p>
    <button type="button" onClick={() => result("normal")}>정상</button>{" "}
    <button type="button" onClick={() => result("preview-only")}>미리보기만</button>{" "}
    <button type="button" onClick={() => result("stays-open")}>계속 열림</button>{" "}
    <button type="button" onClick={() => setText(drawerTraceText())}>기록 읽기</button>
    <p>미리보기에만 보였는지, 손을 뗀 뒤에도 남았는지 함께 알려 준다. 기록은 이 탭에서만 유지된다.</p>
    <textarea aria-label="쓸기 진단 기록" readOnly value={text} onFocus={(event) => event.currentTarget.select()} />
    <button type="button" onClick={() => {
      const blob = new Blob([drawerTraceText()], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url; link.download = "drawer-trace.json"; link.click();
      // 다운로드 사건을 처리한 다음 객체 주소를 해제한다.
      setTimeout(() => URL.revokeObjectURL(url), 0);
    }}>기록 저장</button>
  </details>;
}
