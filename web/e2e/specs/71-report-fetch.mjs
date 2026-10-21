// 실제 host의 보고서 sandbox·CORS·음원 읽기. 기다림은 응답·DOM·오디오 ended 사건이다.
import fs from 'node:fs';
import http from 'node:http';
import { root, aaa, base, browser, webkit, step, pairPage, inList, shot } from "../lib/harness.mjs";

function wav(hz) {
  const count = 22050 / 4;
  const b = Buffer.alloc(44 + Math.floor(count) * 2);
  b.write('RIFF'); b.writeUInt32LE(b.length - 8, 4); b.write('WAVEfmt ', 8);
  b.writeUInt32LE(16, 16); b.writeUInt16LE(1, 20); b.writeUInt16LE(1, 22);
  b.writeUInt32LE(22050, 24); b.writeUInt32LE(44100, 28); b.writeUInt16LE(2, 32); b.writeUInt16LE(16, 34);
  b.write('data', 36); b.writeUInt32LE(b.length - 44, 40);
  for (let n = 0; n < Math.floor(count); n++) b.writeInt16LE(Math.round(Math.sin(n * hz * 2 * Math.PI / 22050) * 16000), 44 + n * 2);
  return b;
}

export default async function () {
  await step("보고서 fetch: Chromium·휴대폰 WebKit에서 자기 음원 두 개 읽기·혼합·재생 끝, 다른 보고서·API·외부·쓰기·쿠키 차단", async () => {
    aaa('project', 'create', 'demo');
    const dir = `${root}/sound-report`;
    fs.mkdirSync(`${dir}/audio`, { recursive: true });
    fs.mkdirSync(`${dir}/nested`);
    fs.writeFileSync(`${dir}/audio/a.wav`, wav(440));
    fs.writeFileSync(`${dir}/audio/b.wav`, wav(660));
    fs.writeFileSync(`${dir}/data.json`, JSON.stringify({ own: 'ok' }));
    fs.writeFileSync(`${dir}/nested/read.html`, '<!doctype html><script>fetch("../data.json").then(r=>r.json()).then(v=>document.body.textContent=v.own)</script><body>대기');
    const otherDir = `${root}/other-report`;
    fs.mkdirSync(otherDir);
    fs.writeFileSync(`${otherDir}/index.html`, 'other secret');
    const other = aaa('library', 'publish', otherDir, '--name', 'other');
    let externalHits = 0;
    const external = http.createServer((req, res) => { externalHits++; res.setHeader('Access-Control-Allow-Origin', '*'); res.end('external'); });
    await new Promise(r => external.listen(0, '127.0.0.1', r));
    const externalUrl = `http://127.0.0.1:${external.address().port}/external`;
    const html = `<!doctype html><html lang=ko><meta charset=utf-8><meta name=viewport content="width=device-width"><h1>음원 혼합 보고서</h1><button id=play>두 음원 재생</button><p id=state>준비</p><script>
      window.result={};
      document.querySelector('#play').onclick=async()=>{
        const ctx=new AudioContext();
        try {
          await ctx.resume();
          const data=await (await fetch('./data.json')).json();
          const buffers=await Promise.all(['a','b'].map(async n=>{
            const r=await fetch('audio/'+n+'.wav'); if(!r.ok) throw Error('음원 '+r.status);
            return ctx.decodeAudioData(await r.arrayBuffer());
          }));
          const analyser=ctx.createAnalyser(); analyser.fftSize=256; const mute=ctx.createGain(); mute.gain.value=0; analyser.connect(mute).connect(ctx.destination);
          let peak=0, done=false;
          const measure=()=>{const v=new Float32Array(256); analyser.getFloatTimeDomainData(v); peak=Math.max(peak,...v.map(Math.abs)); if(!done) requestAnimationFrame(measure);};
          requestAnimationFrame(measure);
          await Promise.all(buffers.map((b,i)=>new Promise(resolve=>{
            const source=ctx.createBufferSource(), gain=ctx.createGain(); source.buffer=b; gain.gain.value=[0.25,0.5][i];
            source.connect(gain).connect(analyser); source.onended=resolve; source.start(ctx.currentTime+0.02);
          })));
          done=true;
          result={own:data.own, decoded:buffers.map(b=>b.length), peak, running:ctx.state, origin:self.origin};
          await ctx.close(); document.querySelector('#state').textContent='재생 끝';
        } catch(e) { document.querySelector('#state').textContent='실패: '+e; await ctx.close(); }
      };
    </script></html>`;
    fs.writeFileSync(`${dir}/index.html`, html);
    const link = aaa('library', 'publish', dir, '--name', 'sound', '--to-user');
    const reportUrl = new URL(link.base, base).href;
    const liveLink = aaa('library', 'publish', dir, '--name', 'sound-live', '--live');
    const safari = await webkit.launch();
    try {
      for (const [name, engine] of [['chromium', browser], ['webkit', safari]]) {
        const paired = await pairPage(engine, aaa('web', 'pair').code, { width: 390, height: 844 });
        try {
          const cookies = await paired.page.context().cookies();
          if (!cookies.some(c => c.httpOnly && c.sameSite === 'Strict')) throw Error('기기 쿠키 보호 누락');
          const reads=[];
          paired.page.on('request', r=>{if(r.url().includes('/audio/')) reads.push(r);});
          await inList(paired.page, 'activity-reports');
          await paired.page.getByTestId('report-demo-sound').getByRole('button', {name:'열기', exact:true}).click();
          const frame=paired.page.frameLocator('[data-testid="report-frame"]');
          await frame.locator('#play').click();
          await frame.locator('#state').filter({hasText:/^(재생 끝|실패:)/}).waitFor();
          if(await frame.locator('#state').innerText()!=='재생 끝') throw Error(await frame.locator('#state').innerText());
          const result=await frame.locator('body').evaluate(()=>window.result);
          console.log(`${name} 390×844 audio: ${JSON.stringify(result)}`);
          if(result.own!=='ok'||result.decoded.length!==2||result.decoded.some(n=>n<5000)||result.peak<0.01||result.running!=='running'||result.origin!=='null') throw Error(`${name} 오디오 ${JSON.stringify(result)}`);
          if(reads.length!==2) throw Error(`${name} 음원 요청 수 ${reads.length}`);
          for(const read of reads) {
            const h=await read.allHeaders();
            if(h.origin!=='null'||h.cookie) throw Error(`${name} opaque 읽기의 origin/cookie`);
            const response=await read.response();
            if(response.headers()['access-control-allow-origin']!=='null'||response.headers()['access-control-allow-credentials']) throw Error('CORS credential 허용');
          }
          const blocked=await frame.locator('body').evaluate(async (_, args)=>{
            const reject=async (url,opts)=>{try {await fetch(url,opts);return false;} catch{return true;}};
            const root=new URL('.',location.href);
            const paths=[args.other, '/api/v1/me', '/api/v1/call', args.external, new URL('../escape.txt',root).href,
              root.href+'%2e%2e/%2e%2e/api/v1/me', root.href+'..%2f..%2fapi/v1/me'];
            const rejected=await Promise.all(paths.map(p=>reject(p)));
            const write=await reject(new URL('data.json',root),{method:'POST',body:'write'});
            const credentials=await reject(new URL('data.json',root),{credentials:'include'});
            const xhr=await new Promise((resolve,reject)=>{const x=new XMLHttpRequest(); x.open('GET',new URL('data.json',root)); x.onload=()=>resolve(JSON.parse(x.responseText).own); x.onerror=reject; x.send();});
            const read=await fetch(new URL('data.json',root),{method:'HEAD'});
            const range=await fetch(new URL('audio/a.wav',root),{headers:{Range:'bytes=0-43'}});
            let cookie=false,storage=false,parent=false;
            try{document.cookie;cookie=true;}catch{}
            try{localStorage.length;storage=true;}catch{}
            try{window.parent.document.body;parent=true;}catch{}
            return {rejected,write,credentials,xhr,head:read.status,range:range.status,rangeSize:(await range.arrayBuffer()).byteLength,cookie,storage,parent};
          },{other:new URL(other.base,base).href,external:externalUrl});
          if(blocked.rejected.some(v=>!v)||!blocked.write||!blocked.credentials||blocked.xhr!=='ok'||blocked.head!==200||blocked.range!==206||blocked.rangeSize!==44||blocked.cookie||blocked.storage||blocked.parent) throw Error(`${name} 격리 ${JSON.stringify(blocked)}`);
          if(externalHits!==0) throw Error('외부 서버에 요청이 닿았다');
          const source=await fetch(reportUrl+'index.html');
          if(await source.text()!==html) throw Error('fetch 원본 HTML에 bridge를 붙였다');
          for(const headers of [{Origin:'null', Cookie:'test=1'}, {Origin:'null'}]) {
            const r=await fetch(new URL('/api/v1/me',base),{headers});
            if(r.status!==403||r.headers.has('access-control-allow-origin')) throw Error('opaque API 응답');
          }
          const preflight=await fetch(reportUrl+'data.json',{method:'OPTIONS',headers:{Origin:'null','Access-Control-Request-Method':'GET','Access-Control-Request-Headers':'x-custom'}});
          if(preflight.status!==403||preflight.headers.has('access-control-allow-origin')) throw Error('preflight 허용');
          const denied=await fetch(reportUrl+'data.json',{headers:{Origin:'null',Cookie:'test=1'}});
          if(denied.status!==403||denied.headers.has('access-control-allow-origin')) throw Error('쿠키 동반 읽기');
          await shot(paired.page, `report-audio-${name}-390`);
          // 공유 링크를 인증 없는 새 탭에서 열어도 같은 읽기와 재생이 된다.
          const outside=await engine.newContext({viewport:{width:390,height:844}});
          try {
            const tab=await outside.newPage(); await tab.goto(reportUrl+'nested/read.html');
            await tab.getByText('ok',{exact:true}).waitFor();
            await tab.goto(new URL(liveLink.base,base).href); await tab.locator('#play').click();
            await tab.locator('#state').filter({hasText:/^(재생 끝|실패:)/}).waitFor();
            if(await tab.locator('#state').innerText()!=='재생 끝') throw Error(await tab.locator('#state').innerText());
          } finally {await outside.close();}
        } finally {await paired.page.context().close();}
      }
      const revoked=aaa('library','link','demo','sound');
      aaa('library','revoke',revoked.id);
      const r=await fetch(new URL(revoked.base+'data.json',base),{headers:{Origin:'null'}});
      if(r.status!==401||r.headers.has('access-control-allow-origin')) throw Error('폐기된 cap가 CORS로 읽힌다');
    } finally {
      await safari.close();
      await new Promise(r=>external.close(r));
    }
  });
}
