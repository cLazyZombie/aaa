import { diagnosticReason } from "./errorDiagnostic.ts";
// 화면 한 부분의 그리기 오류를 그 자리에서 멈춘다(plan §8.266). React는 잡히지 않은 그리기·effect 오류에서 앱 전체를 내려
// 휴대폰 홈 화면 앱이 빈 화면이 된다. 본문·목록·겹쳐 뜨는 창(파일 보기·팔레트·설정)을 따로 감싸 한 곳이 깨져도 나머지와
// host 연결은 남고, 그 자리에 까닭과 「다시 그리기」·「새로고침」을 둔다.
// 참고: t3code(MIT) `apps/web/src/components/RenderErrorBoundary.tsx`(1451e7cb2)의 class 경계와 `resetKeys`(보는 것이 바뀌면
// 저절로 다시 그린다). 다른 곳: 참고는 fallback을 부르는 쪽이 주고, 여기는 한 모양(까닭·두 단추·있으면 닫기)이다.
import { Component, type ErrorInfo, type ReactNode } from "react";
import { RotateCcw, TriangleAlert, X } from "lucide-react";

interface Props {
  /** 깨진 자리의 이름(「본문」·「목록」·「파일 보기」): 글과 시험 표지 */
  area: string;
  /** 이 값들이 바뀌면(다른 pane·화면으로 감) 오류를 잊고 다시 그린다 */
  resetKeys?: readonly unknown[];
  /** 겹쳐 뜨는 창이면 닫는 길 */
  onDismiss?: () => void;
  children: ReactNode;
}

interface State {
  error: Error | null;
  keys: readonly unknown[] | undefined;
}

/** 받침이 있으면 「을」, 없으면 「를」(「본문을」·「파일 보기를」) */
export function objectParticle(word: string): string {
  const code = word.charCodeAt(word.length - 1) - 0xac00;
  return code >= 0 && code < 11172 && code % 28 !== 0 ? "을" : "를";
}

const same = (a: readonly unknown[] | undefined, b: readonly unknown[] | undefined) =>
  a === b || (a !== undefined && b !== undefined && a.length === b.length && a.every((v, i) => Object.is(v, b[i])));

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null, keys: this.props.resetKeys };

  static getDerivedStateFromError(error: unknown): Partial<State> {
    return { error: error instanceof Error ? error : new Error(String(error)) };
  }

  static getDerivedStateFromProps(props: Props, state: State): Partial<State> | null {
    if (same(props.resetKeys, state.keys)) return null;
    return { keys: props.resetKeys, error: null };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error(`[aaa] ${this.props.area}${objectParticle(this.props.area)} 그리지 못했다`, diagnosticReason(error), info.componentStack);
  }

  render() {
    const { error } = this.state;
    if (error === null) return this.props.children;
    return (
      <div className="render-error" role="alert" data-testid="render-error" data-area={this.props.area}>
        <TriangleAlert aria-hidden="true" />
        <p className="render-error-title">{this.props.area}{objectParticle(this.props.area)} 그리지 못했다</p>
        <p className="render-error-detail">{error.message || error.name}</p>
        <p className="render-error-hint">다른 부분과 host 연결은 그대로다. 다시 그려도 같으면 새로고침한다.</p>
        <div className="render-error-actions">
          <button type="button" className="btn btn-primary" onClick={() => this.setState({ error: null })}>
            <RotateCcw aria-hidden="true" />
            다시 그리기
          </button>
          <button type="button" className="btn" onClick={() => location.reload()}>
            새로고침
          </button>
          {this.props.onDismiss && (
            <button type="button" className="btn" onClick={this.props.onDismiss}>
              <X aria-hidden="true" />
              닫기
            </button>
          )}
        </div>
      </div>
    );
  }
}
