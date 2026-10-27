// host 작업 기록 검색. 본문은 host가 반환한 내용 해시의 판만 연다.
import { useEffect, useRef, useState } from "react";
import { Search, ArrowLeft, ExternalLink } from "lucide-react";
import { call } from "./api";
import type { RecordDocument, RecordHit, RecordKind, RecordSearchPage } from "./gen/proto";
import { diagnosticReason } from "./errorDiagnostic";
import { Markdown } from "./Markdown";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import "./RecordSearch.css";

const LABEL: Record<RecordKind,string> = { report: "완료 보고", question: "답한 질문", task: "할 일" };
const date = (ms:number) => ms ? new Date(ms).toLocaleString("ko-KR") : "시각 미상";

export function RecordSearch({ shown }: { shown:boolean }) {
  const [query,setQuery]=useState("");
  const [kind,setKind]=useState<RecordKind | "">("");
  const [page,setPage]=useState<RecordSearchPage | null>(null);
  const [busy,setBusy]=useState(false);
  const [problem,setProblem]=useState("");
  const [doc,setDoc]=useState<RecordDocument | null>(null);
  const [opening,setOpening]=useState<RecordHit | null>(null);
  const request=useRef(0);
  const active=useRef<{query:string;kind:RecordKind | ""} | null>(null);
  useEffect(()=>()=>{request.current++;},[]);
  const invalidate=()=>{request.current++;active.current=null;setPage(null);setBusy(false);setProblem("");};
  const search=async (more=false) => {
    const seq=++request.current;
    const current=more ? active.current : {query:query.trim(),kind};
    if (!current?.query) { setPage(null); setBusy(false); return; }
    if (!more) {active.current=current;setPage(null);}
    setDoc(null);setOpening(null);setBusy(true);setProblem("");
    try {
      const r=await call({op:"record_search",query:{ query:current.query,kind:current.kind || null,project:null,limit:30,after:more ? page?.next ?? null : null }});
      if (seq===request.current && r.kind==="record_search") {
        setPage(old=>({ ...r,hits:more ? [...(old?.hits ?? []),...r.hits] : r.hits }));
      }
    } catch(e) {if(seq===request.current)setProblem(diagnosticReason(e));}
    finally {if(seq===request.current)setBusy(false);}
  };
  const open=async(hit:RecordHit) => {
    const seq=++request.current;
    setOpening(hit);setDoc(null);setBusy(true);setProblem("");
    try {
      let offset:number | undefined;
      let full:RecordDocument | null=null;
      do {
        const r=await call({op:"record_open",project:hit.project,key:hit.key,offset});
        if(seq!==request.current)return;
        if(r.kind!=="record_document")throw new Error("원문 응답 형식이 다릅니다.");
        full=full ? {...r,body:full.body+r.body} : r;
        offset=r.next ?? undefined;
      } while(offset!==undefined);
      setDoc(full);
    } catch(e) {if(seq===request.current)setProblem(diagnosticReason(e));}
    finally {if(seq===request.current)setBusy(false);}
  };
  const back=()=>{request.current++;setDoc(null);setOpening(null);setBusy(false);setProblem("");};
  const selected=doc?.hit ?? opening;
  return <div className={`thread-view record-search${shown ? "" : " is-hidden"}`} inert={!shown} data-testid="record-search" data-shown={shown}>
    <header className="term-head"><DrawerToggle/><HeadNote/>{opening && <button className="ghost icon" aria-label="검색 결과로" onClick={back}><ArrowLeft/></button>}<div className="term-title"><strong>{opening ? opening.title : "기록 검색"}</strong></div><HeadMore/></header>
    <div className="thread-body"><div className="thread-col">
      {!opening && <>
        <form className="record-search-form" onSubmit={e=>{e.preventDefault();void search();}}>
          <label className="record-query"><Search aria-hidden="true"/><input className="input" type="search" aria-label="작업 기록 검색어" placeholder="기록 본문 검색" value={query} onChange={e=>{invalidate();setQuery(e.target.value);}}/></label>
          <select className="input" aria-label="기록 종류" value={kind} onChange={e=>{invalidate();setKind(e.target.value as RecordKind | "");}}><option value="">모든 기록</option>{Object.entries(LABEL).map(([k,v])=><option key={k} value={k}>{v}</option>)}</select>
          <button className="primary" type="submit" disabled={busy || !query.trim()}>검색</button>
        </form>
        <div className="record-results" aria-label="검색 결과" aria-busy={busy}>
          {page?.hits.map(h=><button className="record-hit" key={`${h.project}/${h.key}`} onClick={()=>void open(h)} data-testid="record-hit">
            <span className="record-meta">{h.project} · {LABEL[h.kind]} · {h.id} · {date(h.at_ms)}</span><strong>{h.title}</strong><span className="record-excerpt">{h.excerpt}</span>
          </button>)}
          {page?.hits.length===0 && <p role="status">일치하는 기록이 없습니다.</p>}
        </div>
        {page?.next && <button className="ghost" aria-label="검색 결과 더 보기" disabled={busy} onClick={()=>void search(true)}>더 보기</button>}
      </>}
      {selected && <div className="record-original" data-testid="record-original">
        <p className="record-meta">{selected.project} · {LABEL[selected.kind]} · {selected.id} · {date(selected.at_ms)}</p>
        {selected.report_id && <a href={`#report=${encodeURIComponent(`${selected.project}/${selected.report_id}`)}`} className="record-report-link"><ExternalLink aria-hidden="true"/>{selected.report_id} 보고 판</a>}
        {doc && <Markdown>{doc.body}</Markdown>}
      </div>}
      {busy && <p role="status">불러오는 중</p>}
      {problem && <p role="alert">{problem}</p>}
    </div></div>
  </div>;
}
