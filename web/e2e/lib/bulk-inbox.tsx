// 현재 앱 라우팅에서 숨겨진 컴포넌트도 실제 host API로 검증한다. 배포 번들에는 넣지 않는다.
import { createRoot } from "react-dom/client";
import { InboxPanel } from "../../src/InboxPanel";
import "../../src/style.css";

createRoot(document.getElementById("root")!).render(<InboxPanel canControl />);
