// 실제 host의 보관 원문과 CLI·웹. 검색 결과는 최신 본문이 아닌 일치한 판을 연다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { aaa, root, step, browser, webkit, pairPage, domIs, inList, shot } from "../lib/harness.mjs";

const oldTask="- [ ] 검색할 일 <!-- aaa-task:01ARZ3NDEKTSV4RRFFQ69G5FAV -->\n  공통자료 예전메모 SQLite\n";
const newTask=oldTask.replace("예전메모 SQLite","현재메모 파일");
const reportBody=i=>`[완료] 검색 보고 ${i}\n\n공통자료 ${i===1 ? "과거원문 SQLite" : "기록 내용"}\n`;
const request=(page,value)=>page.evaluate(async value=>(await fetch("/api/v1/call",{method:"POST",headers:{"content-type":"application/json"},body:JSON.stringify(value)})).json(),value);
let safari;

export default async function () {
  await step("기록 검색 준비: host 보관 판·답·할 일 이력을 만들고 CLI로 일치 원문을 연다",async()=>{
    aaa("project","create","demo");
    const dir=`${root}/projects/demo`;
    fs.writeFileSync(`${dir}/TASKS.md`,`## Backlog\n${newTask}`);
    const db=new DatabaseSync(`${root}/s/host/host.db`);
    try {
      db.exec("BEGIN");
      for(let i=1;i<=34;i++) {
        const id=`r-${String(i).padStart(4,"0")}`;
        const source=i<=2 ? "t-0001" : `t-${String(i).padStart(4,"0")}`;
        db.prepare("INSERT INTO report_ids(project,seq,id,source,revision,ready,deleted) VALUES('demo',?,?,?,?,1,0)").run(i,id,source,`v${i}`);
        db.prepare("INSERT INTO library_reports(project,name,updated,to_user,from_thread) VALUES('demo',?,?,0,?)").run(id,Date.UTC(2026,9,20,3,0)+i*1000,source);
        fs.writeFileSync(`${dir}/threads/${id}.md`,reportBody(i));
      }
      const answer={picks:[0],comments:["공통자료 직접 쓴 결정"],text:null,summary:"1A 직접",by:"CLI",at_ms:Date.now()};
      const record={id:"search-answer",kind:"question",subject:"t-0001",title:"검색 질문",asked_ms:Date.now(),done_ms:Date.now(),by:"CLI",summary:"1A 직접",report_id:"r-0001",question_id:"q-0001",items:[{id:"q-0001",text:"검색 질문",body:"공통자료 당시 질문본문",choices:[{label:"과거판",recommended:true,description:"보존",body:null}]}],answer};
      db.prepare("INSERT INTO asks(project,id,kind,subject,done_ms,data) VALUES('demo',?,'question','t-0001',?,?)").run(record.id,record.done_ms,JSON.stringify(record));
      db.prepare("INSERT INTO task_log(project,at_ms,actor,action,list,title,before,after) VALUES('demo',100,'file','update','Backlog','검색할 일',?,?)").run(oldTask,newTask);
      db.exec("COMMIT");
    } finally { db.close(); }
    const found=aaa("search","sqlite");assert.equal(found.hits.length,2);
    const hit=found.hits.find(h=>h.kind==="report");
    assert.equal(aaa("record",hit.project,hit.key).body,reportBody(1));
    assert.equal(aaa("search","q-00001","--kind","question").hits.length,1);
    assert.equal(aaa("search","예전메모","--kind","task").hits.length,1);
  });

  async function scenario(engine,width,reader,label) {
    const paired=await pairPage(engine,aaa("web","pair",...(reader ? ["--reader"] : [])).code,{width,height:844});
    const {page,errors}=paired;
    try {
      await inList(page,"activity-search");
      const query=page.getByRole("searchbox",{name:"작업 기록 검색어"});
      await query.fill("공통자료");await query.press("Enter");
      await domIs(page,"통합 검색 첫 페이지",()=>document.querySelectorAll('[data-testid="record-hit"]').length===30);
      assert.ok(!(await page.locator(".record-results").innerText()).includes("aaa-task:"),"숨은 할 일 표식은 결과 미리보기에 노출하지 않는다");
      await shot(page,`record-results-${label}`);
      await page.getByRole("button",{name:"검색 결과 더 보기",exact:true}).click();
      await domIs(page,"통합 검색 다음 페이지",()=>document.querySelectorAll('[data-testid="record-hit"]').length===37);
      const layout=await page.getByTestId("record-hit").evaluateAll(rows=>rows.every((row,i)=>{
        const r=row.getBoundingClientRect();
        const next=rows[i+1]?.getBoundingClientRect();
        return (!next || r.bottom<=next.top) && [...row.children].every(child=>{
          const c=child.getBoundingClientRect();
          return c.top>=r.top && c.bottom<=r.bottom && c.left>=r.left && c.right<=r.right;
        });
      }));
      assert.ok(layout,"결과 본문은 자기 단추 안에 있고 다음 결과와 겹치지 않는다");
      await page.getByTestId("record-hit").filter({hasText:"검색 보고 1"}).filter({hasText:"과거원문"}).click();
      await page.getByTestId("record-original").getByText("공통자료 과거원문 SQLite",{exact:true}).waitFor();
      await shot(page,`record-original-${label}`);
      assert.equal(await page.locator('.record-original').evaluate(el=>el.scrollWidth>el.clientWidth+1),false);
      await page.getByRole("button",{name:"검색 결과로",exact:true}).click();
      await page.getByRole("combobox",{name:"기록 종류"}).selectOption("question");
      await query.fill("직접 쓴 결정");await query.press("Enter");
      await page.getByTestId("record-hit").click();
      await page.getByTestId("record-original").getByText("공통자료 직접 쓴 결정",{exact:true}).waitFor();
      assert.ok((await page.getByTestId("record-original").innerText()).includes("당시 질문본문"));
      const api=await request(page,{op:"record_search",query:{query:"예전메모",kind:"task",limit:1}});
      assert.ok(api.ok);assert.equal(api.ok.hits.length,1);
      const original=await request(page,{op:"record_open",project:"demo",key:api.ok.hits[0].key});
      assert.equal(original.ok.body,oldTask);
      assert.deepEqual(errors,[]);
    } finally {await page.close();}
  }
  await step("기록 검색 Chromium: 종류·다음 페이지·당시 원문과 직접 답을 연다",async()=>scenario(browser,1280,false,"chromium"));
  await step("기록 검색 WebKit: 휴대폰 Reader도 같은 보관 판을 읽는다",async()=>{
    safari=await webkit.launch({headless:true});
    try {await scenario(safari,390,true,"webkit-390");} finally {await safari.close();}
  });
}
