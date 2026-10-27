// 실제 React 카드·CSS의 headless 회귀. host/PTY 전달은 기존 E53이 맡는다.
// node web/e2e/question-comments.browser.mjs [그림 폴더] [--before-ref=<git ref>]
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";
import react from "@vitejs/plugin-react";
import ts from "typescript-api";
import { chromiumLaunchOptions } from "./lib/chromium-launch.mjs";

const require = createRequire(path.join(process.env.NODE_PATH ?? process.cwd(), "noop.js"));
const { chromium } = require("playwright");
const web = fileURLToPath(new URL("../", import.meta.url));
const shots = process.argv.slice(2).find(a => !a.startsWith("--"));
const before = process.argv.find(a => a.startsWith("--before-ref="))?.split("=")[1];
const moduleId = "\0question-comments.tsx";
const oldId = "\0before-asks.tsx";
const compile = source => ts.transpileModule(source, {compilerOptions:{jsx:ts.JsxEmit.ReactJSX,module:ts.ModuleKind.ESNext}}).outputText;
const source = `
import React, {useState} from 'react';
import {createRoot} from 'react-dom/client';
import {flushSync} from 'react-dom';
import {AsksView} from '${before ? "virtual:before-asks" : "/src/AsksView.tsx"}';
import '/src/herdr/tokens.css';
import '/src/herdr/modal.css';
import '/src/style.css';
import '/src/herdr/primitives.css';
const params = new URLSearchParams(location.search);
document.documentElement.dataset.theme = params.get('theme') || 'light';
const item = (n, text) => ({source_number:n, id:'q-'+(1000+n), text, choices:[
  {label:'오른쪽에 둔다',recommended:true}, {label:'가운데에 둔다',recommended:false}
]});
const items = params.has('single') ? [item(3,'로그인 단추 자리')] : [item(3,'로그인 단추 자리'),item(7,'새 알림 표시')];
if(params.has('no-recommend')) items[1].choices.forEach(c=>c.recommended=false);
const q = {report_hash:'h', items, answer:null, draft:params.has('draft') ? {picks:[0,null],text:null,comments:['이전 의견',null]} : null};
const thread = {id:'t-1000', title:'질문별 의견', project:'demo', questions:q};
const request = {id:'request',title:'질문별 의견',created_ms:1,questions:q};
const noop=async()=>{};
function App(){
  const [done,setDone]=useState(null);
  const [nonce,setNonce]=useState(0);
  window.bump=()=>flushSync(()=>setNonce(n=>n+1));
  const answer=async(subject,picks,text,comments)=>{
    window.answerCalls=(window.answerCalls||0)+1;
    if(params.has('reject'))throw new Error('질문이 바뀌었다');
    window.sent={picks,text,comments};
    setDone({id:'record',kind:params.has('request')?'request':'question',subject:'t-1000',title:'질문별 의견',items,
      answer:{picks,text,comments,summary:'답함',by:'웹',at_ms:1},asked_ms:1,done_ms:1,by:'웹',summary:'답함'});
  };
  const ask = params.has('request') ? {key:'r:request',kind:'request',request} : {key:'q:t-1000',kind:'question',thread};
  return <div className="view" data-nonce={nonce} style={{height:'100dvh'}}><AsksView shown onClose={noop}
    selection={done?{kind:'done',record:done}:{kind:'open',ask}}
    canControl={!params.has('reader')} onAnswer={answer} onRequestAnswer={answer}
    onHandle={noop} onReply={noop} onForget={noop} onOpenTask={noop} onThread={noop}/></div>;
}
createRoot(document.getElementById('root')).render(<App/>);
`;
const server = await createServer({
  root: web, configFile: false, publicDir: path.join(web, "public"),
  server: { host: "127.0.0.1", port: 0 },
  plugins: [react(), {
    name: "question-comments-fixture",
    resolveId(id) { if (id === "virtual:question-comments") return moduleId; if (id === "virtual:before-asks") return oldId; },
    load(id) {
      if (id === moduleId) return compile(source);
      if (id === oldId) return compile(execFileSync("git", ["show", `${before}:web/src/AsksView.tsx`], { cwd: web, encoding: "utf8" })
        .replaceAll('from "./', 'from "/src/').replaceAll('import "./', 'import "/src/'));
    },
    configureServer(server) {
      server.middlewares.use((req,res,next)=>{
        if (!req.url.startsWith('/__questions')) return next();
        res.setHeader('content-type','text/html');
        const html = '<html><head><meta name="viewport" content="width=device-width,initial-scale=1"></head><body><div id="root"></div><script type="module">import "virtual:question-comments"</script></body></html>';
        void server.transformIndexHtml(req.url, html).then(body => res.end(body), error => { res.statusCode = 500; res.end(String(error)); });
      });
    },
  }],
});
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ ...chromiumLaunchOptions(), ...(process.env.AAA_BROWSER_CHANNEL ? {channel:process.env.AAA_BROWSER_CHANNEL} : {}) });
  const base = `http://127.0.0.1:${server.httpServer.address().port}/__questions`;
  for (const width of [1280,390]) for (const theme of ["light","dark"]) {
    const started = performance.now();
    const page = await browser.newPage({viewport:{width,height:1000}});
    page.setDefaultTimeout(5000);
    const errors=[]; page.on('pageerror',e=>{ errors.push(e.message); console.error(e.message); });
    await page.goto(`${base}?theme=${theme}`);
    const card=page.locator('.ask-card'); await card.waitFor();
    if (shots) {
      fs.mkdirSync(shots,{recursive:true});
      await card.screenshot({path:path.join(shots,`${before?'before':'after'}-${width}-${theme}.png`)});
    }
    if (!before) {
      assert.equal(await page.getByTestId('thread-ask-text').count(),0);
      for (const n of [3,7]) assert.equal(await page.getByTestId(`thread-ask-text-${n}`).count(),1);
      await page.getByTestId('thread-ask-pick-3A').click();
      await page.evaluate(()=>document.getElementById('root').addEventListener('input',event=>{
        if(event.target.tagName==='TEXTAREA') window.bump();
      },true));
      await page.getByTestId('thread-ask-text-3').fill('右側 유지\n간격은 넓게');
      await page.evaluate(()=>window.bump());
      assert.equal(await page.getByTestId('thread-ask-text-3').inputValue(),'右側 유지\n간격은 넓게');
      await page.getByTestId('thread-ask-text-7').fill('둘 다 보이게');
      await page.getByTestId('thread-ask-send').click();
      await page.getByTestId('ask-done-text-7').waitFor();
      assert.deepEqual(await page.evaluate(()=>window.sent),{picks:[0,null],text:null,comments:['右側 유지\n간격은 넓게','둘 다 보이게']});
      assert.equal(await page.getByTestId('ask-done-text-3').innerText(),'직접 쓴 답 右側 유지\n간격은 넓게');
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
    }
    assert.deepEqual(errors,[]);
    console.log(`${before?'before':'after'} ${width} ${theme}: ${Math.round(performance.now()-started)}ms`);
    await page.close();
  }
  if (!before) for (const scenario of ['single','reader','no-recommend','draft','request','reject']) {
    const page=await browser.newPage(); page.setDefaultTimeout(5000);
    await page.goto(`${base}?${scenario}`); await page.locator('.ask-card').waitFor();
    if(scenario==='single') {
      await page.getByTestId('thread-ask-text').fill('한 질문 원문');
      await page.getByTestId('thread-ask-text').press('Enter');
      await page.getByTestId('ask-done-text').waitFor();
      assert.deepEqual(await page.evaluate(()=>window.sent),{picks:[null],text:'한 질문 원문',comments:[]});
    } else if(scenario==='reader') {
      for(const node of await page.locator('textarea,[role=radio],[data-testid=thread-ask-send]').all())assert.equal(await node.isDisabled(),true);
    } else if(scenario==='no-recommend') {
      await page.getByTestId('thread-ask-text-3').fill('첫 질문만 답함');
      assert.equal(await page.getByTestId('thread-ask-send').isDisabled(),true);
    } else if(scenario==='draft') {
      assert.equal(await page.getByTestId('thread-ask-text-3').inputValue(),'이전 의견');
    } else {
      await page.getByTestId('thread-ask-text-3').fill('첫째 의견');
      await page.getByTestId('thread-ask-send').click();
      if(scenario==='reject') {
        await page.getByRole('alert').waitFor();
        assert.equal(await page.getByTestId('thread-ask-text-3').inputValue(),'첫째 의견');
        assert.equal(await page.getByTestId('thread-ask-send').isEnabled(),true);
        assert.equal(await page.evaluate(()=>window.answerCalls),1);
      } else {
        await page.getByTestId('ask-done-text-3').waitFor();
        assert.deepEqual(await page.evaluate(()=>window.sent),{picks:[null,0],text:null,comments:['첫째 의견',null]});
      }
    }
    console.log(`scenario ${scenario}: ok`); await page.close();
  }
} finally { await browser?.close(); await server.close(); }
