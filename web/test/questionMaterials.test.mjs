import test from "node:test";
import assert from "node:assert/strict";
import { questionMaterials } from "../src/questionMaterials.ts";
const origin="https://example.ts.net";
const material=(body,more={})=>questionMaterials({origin,items:[{text:"검수",body,choices:[]}],...more});
test("같은 보고 r 참조를 본문·표·목록에서 찾아 한 번만 머리에 놓는다",()=>{
 const got=material('[보고서](aaa:report/r-0001)\n\n| 결과 |\n| --- |\n| [판](https://example.ts.net/#report=demo%2Fr-0001) |\n\n- aaa:report/r-0002',{project:"demo",reportId:"r-0001"});
 assert.deepEqual(got.map((r)=>[r.kind,r.name]),[["report","r-0001"],["report","r-0002"]]);
});
test("그림은 질문 본문에 남기고 음원·자료 파일만 안전한 질문 파일 경로로 연다",()=>{
 const got=material('![시안](library/a.svg)\n[음원](library/preview/theme.wav)\n[HTML](library/preview/index.html)',{base:"/api/question/hash/"});
 assert.deepEqual(got.map((r)=>r.url),["/api/question/hash/library/preview/theme.wav","/api/question/hash/library/preview/index.html"]);
});
test("코드 예시·API·로컬 경로·실행 scheme은 머리 링크로 만들지 않는다",()=>{
 assert.deepEqual(material('`aaa:report/r-0001`\n```\naaa:report/r-0002\n```\n[API](https://example.ts.net/api/v1/me)\n[로컬](/tmp/report.html)\n[실행](javascript:alert)\n[벗어남](library/../private.html)',{base:"/api/question/hash/"}),[]);
 const got=material('[웹 보고서](https://reports.example.com/preview.html)');
 assert.equal(got[0].url,"https://reports.example.com/preview.html");
 assert.equal(material("[전체 보고서](aaa:report/r-0001)")[0].label,"전체 보고서");
});
