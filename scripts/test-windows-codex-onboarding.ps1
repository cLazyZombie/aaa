#requires -Version 7
# 실제 Codex + 격리 host/agent/worktree에서 최초·재시작 brief와 승인 없는 명령을 확인한다.
# 운영 서비스·인증·개인 Codex 설정을 쓰지 않는다. 실행자가 기준 목록을 저장·대조한다.
param(
    [Parameter(Mandatory)][string]$Aaa,
    [Parameter(Mandatory)][string]$Codex,
    [Parameter(Mandatory)][ValidatePattern('^C:\\aaa-test\\t-[0-9]+$')][string]$Lab
)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$Aaa=[IO.Path]::GetFullPath($Aaa);$Codex=[IO.Path]::GetFullPath($Codex)
foreach($path in @($Aaa,$Codex)) {
    if(-not $path.StartsWith($Lab+'\',[StringComparison]::OrdinalIgnoreCase)){throw '시험 폴더 안의 실행 파일만 허용한다'}
}
$run=Join-Path $Lab 'codex-onboarding'
if(Test-Path -LiteralPath $run){throw '시험 결과 폴더가 이미 있다'}
[void][IO.Directory]::CreateDirectory($run)
$env:TEMP=$run;$env:TMP=$run
$env:AAA_CONFIG_DIR=Join-Path $run 'config';$env:AAA_STATE_DIR=Join-Path $run 'state'
$env:AAA_INSTANCE='aaa-test-'+(Split-Path $Lab -Leaf).Substring(2)+'-c'
foreach($key in @('AAA_SESSION_ID','AAA_HOOK_SOCK','AAA_HOOK_TOKEN','AAA_PROJECT','AAA_THREAD','CODEX_HOME','OPENAI_API_KEY','OPENAI_BASE_URL')) {
    [Environment]::SetEnvironmentVariable($key,$null,'Process')
}
[void][IO.Directory]::CreateDirectory($env:AAA_CONFIG_DIR)
# HTTP는 loopback 임의 포트만 열며 URL ACL·방화벽·외부 API를 쓰지 않는다.
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.Json.Nodes;
using System.Threading;
using System.Threading.Tasks;
public sealed class CodexFixture : IDisposable {
 readonly TcpListener listener = new TcpListener(IPAddress.Loopback, 0);
 public readonly AutoResetEvent Completed = new AutoResetEvent(false);
 public string Error;
 public int Commands;
 int calls;
 readonly string root;
 bool disposed;
 public int Port { get { return ((IPEndPoint)listener.LocalEndpoint).Port; } }
 static string Json(object value) { return System.Text.Json.JsonSerializer.Serialize(value); }
 static string Event(object value) { return "data: " + Json(value) + "\n\n"; }
 public CodexFixture(string directory) { root=directory;listener.Start(); Task.Run(async () => {
  try { while(true) { var client=await listener.AcceptTcpClientAsync(); await Handle(client); } }
  catch(ObjectDisposedException) when(disposed) {} catch(SocketException) when(disposed) {} catch(Exception e) { Error=e.ToString(); Completed.Set(); }
 }); }
 async Task Handle(TcpClient client) {
  using(client) using(var stream=client.GetStream()) {
   var header=new MemoryStream(); int b;
   while((b=stream.ReadByte())!=-1) {
    header.WriteByte((byte)b);
    var a=header.GetBuffer();int n=(int)header.Length;
    if(n>=4 && a[n-4]==13 && a[n-3]==10 && a[n-2]==13 && a[n-1]==10) break;
    if(n>65536) throw new Exception("HTTP header too large");
   }
   string headers=Encoding.ASCII.GetString(header.ToArray()); int length=0;
   foreach(var line in headers.Split('\n')) {
    if(line.StartsWith("Content-Length:",StringComparison.OrdinalIgnoreCase)) length=int.Parse(line.Split(':',2)[1].Trim());
    if(line.StartsWith("Transfer-Encoding:",StringComparison.OrdinalIgnoreCase)) throw new Exception("unexpected chunked request");
   }
   if(length>8*1024*1024) throw new Exception("HTTP body too large");
   byte[] body=new byte[length]; await stream.ReadExactlyAsync(body);
   string payload;
   string contentType;
   if(headers.StartsWith("POST ") && headers.Split('\r')[0].Contains("/responses")) {
    calls++;var request=JsonNode.Parse(body);var tools=request["tools"].AsArray();
    var outputs=new JsonArray();
    foreach(var item in request["input"].AsArray()) {
     if(item?["type"]?.GetValue<string>()=="function_call_output") outputs.Add(item.DeepClone());
    }
    // 인증 header·prompt·사용자 skill 목록은 남기지 않고 실제 도구 결과만 진단한다.
    File.WriteAllText(Path.Combine(root,"request-"+calls+".json"),Json(new {tools=tools.Count,function_outputs=outputs}));
    if(tools.Count==0) {
     // Codex는 같은 provider로 제목 생성도 병행한다. 명령 결과와 구별한다.
     payload=Event(new {type="response.output_item.done",item=new {type="message",role="assistant",id="title_fixture_"+calls,content=new[]{new {type="output_text",text="격리 파일 확인"}}}})
      +Event(new {type="response.completed",response=new {id="resp_fixture_"+calls}});
    } else if(outputs.Count==0) {
     string tool=null;string space=null;
     foreach(var item in tools) {
      var name=item?["name"]?.GetValue<string>();
      if(name=="exec_command" || name=="shell_command") {tool=name;break;}
      if(item?["type"]?.GetValue<string>()=="namespace") {
       foreach(var function in item["tools"].AsArray()) {
        var nested=function?["name"]?.GetValue<string>();
        if(nested=="exec_command" || nested=="shell_command") {tool=nested;space=name;break;}
       }
       if(tool!=null) break;
      }
     }
     if(tool==null) throw new Exception("no shell tool in actual Codex request");
     string command="$env:PATH='C:\\Program Files\\Git\\cmd;'+$env:PATH; Get-Content -LiteralPath seed.txt; if (-not $?) { exit 1 }; git status --porcelain; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; Write-Output AAA_CODEX_GIT_OK; Write-Output AAA_CODEX_COMMAND_OK";
     string args=tool=="exec_command" ? Json(new {cmd=command,max_output_tokens=1000,login=false}) : Json(new {command=command});
     payload=Event(new {type="response.created",response=new {id="resp_fixture_"+calls}})
      +Event(new {type="response.output_item.done",item=new {type="function_call",call_id="call_fixture_"+calls,name=tool,@namespace=space,arguments=args}})
      +Event(new {type="response.completed",response=new {id="resp_fixture_"+calls}});
    } else {
     bool found=false;
     foreach(var item in request["input"].AsArray()) {
      if(item?["type"]?.GetValue<string>()=="function_call_output") {
       string output=item["output"].ToJsonString();
       if(output.Contains("AAA_CODEX_FILE_OK") && output.Contains("AAA_CODEX_COMMAND_OK") && output.Contains("AAA_CODEX_GIT_OK")) found=true;
      }
     }
     if(!found) throw new Exception("shell result missing Get-Content/git/command proof; see request-"+calls+".json");
     Commands++;
     payload=Event(new {type="response.output_item.done",item=new {type="message",role="assistant",id="msg_fixture_"+calls,content=new[]{new {type="output_text",text="AAA_CODEX_SMOKE_OK"}}}})
      +Event(new {type="response.completed",response=new {id="resp_fixture_"+calls}});
     Completed.Set();
    }
    contentType="text/event-stream";
   } else { payload="{\"data\":[]}";contentType="application/json"; }
   byte[] response=Encoding.UTF8.GetBytes(payload);
   byte[] prefix=Encoding.ASCII.GetBytes("HTTP/1.1 200 OK\r\nContent-Type: "+contentType+"\r\nContent-Length: "+response.Length+"\r\nConnection: close\r\n\r\n");
   await stream.WriteAsync(prefix);await stream.WriteAsync(response);await stream.FlushAsync();
  }
 }
 public void Dispose() { disposed=true;listener.Stop(); }
}
'@
$server=[CodexFixture]::new($run)
$processes=[Collections.Generic.List[Diagnostics.Process]]::new()
function Start-ProcessOwned([string]$Exe,[string[]]$Arguments) {
    $p=[Diagnostics.Process]::new();$p.StartInfo.FileName=$Exe
    foreach($arg in $Arguments){$p.StartInfo.ArgumentList.Add($arg)}
    $p.StartInfo.UseShellExecute=$false;$p.StartInfo.CreateNoWindow=$true
    $p.StartInfo.RedirectStandardOutput=$true;$p.StartInfo.RedirectStandardError=$true
    $p.StartInfo.StandardOutputEncoding=[Text.UTF8Encoding]::new($false)
    $p.StartInfo.StandardErrorEncoding=[Text.UTF8Encoding]::new($false)
    # PowerShell 7/.NET는 null 설정 뒤 빈 환경 항목을 남길 수 있다. CLI의 세션 밖 검사는 존재 여부다.
    foreach($key in @('AAA_SESSION_ID','AAA_HOOK_SOCK','AAA_HOOK_TOKEN','AAA_PROJECT','AAA_THREAD','CODEX_HOME','OPENAI_API_KEY','OPENAI_BASE_URL')) {
        [void]$p.StartInfo.Environment.Remove($key)
    }
    [void]$p.Start();$processes.Add($p);return $p
}
function Invoke-Aaa([string[]]$Arguments) {
    $p=Start-ProcessOwned $Aaa (@('--json')+$Arguments)
    $output=$p.StandardOutput.ReadToEndAsync();$errors=$p.StandardError.ReadToEndAsync()
    if(-not $p.WaitForExit(30000)){throw 'CLI 종료 사건 시한 초과'}
    $text=$output.GetAwaiter().GetResult();$errorText=$errors.GetAwaiter().GetResult()
    if($p.ExitCode -ne 0){throw ('CLI '+($Arguments -join ' ')+': '+$errorText+' '+$text)}
    return $text | ConvertFrom-Json
}
function Wait-Ready($Process,[string]$Match,[string]$Name) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    while($true) {
        $line=$Process.StandardError.ReadLineAsync()
        if(-not $line.Wait([int][Math]::Max(1,20000-$watch.ElapsedMilliseconds))){throw ($Name+' 준비 사건 시한 초과')}
        $text=$line.GetAwaiter().GetResult();if($null -eq $text){throw ($Name+' 준비 전 EOF')}
        [IO.File]::AppendAllText((Join-Path $run ($Name+'.log')),$text+"`n")
        if($text.Contains($Match)){return}
    }
}
$hostProcess=$null;$agentProcess=$null;$thread=$null;$panes=[Collections.Generic.List[string]]::new()
try {
    $git='C:\Program Files\Git\cmd\git.exe'
    $origin=Join-Path $run 'origin repo';[void][IO.Directory]::CreateDirectory($origin)
    foreach($gitArgs in @(@('init','-b','main'),@('config','user.name','Example'),@('config','user.email','test@example.com'))) {
        & $git -C $origin @gitArgs | Out-Null;if($LASTEXITCODE -ne 0){throw 'fixture git 초기화 실패'}
    }
    [IO.File]::WriteAllText((Join-Path $origin 'seed.txt'),'AAA_CODEX_FILE_OK')
    & $git -C $origin add .;if($LASTEXITCODE -ne 0){throw 'fixture git add 실패'}
    & $git -C $origin commit -m fixture | Out-Null;if($LASTEXITCODE -ne 0){throw 'fixture git commit 실패'}
    $env:PATH=(Split-Path $git -Parent)+';'+$env:PATH
    $projects=(Join-Path $run 'projects').Replace('\','/');$work=(Join-Path $run 'work').Replace('\','/')
    $provider='model_providers.fixture={name="fixture",base_url="http://127.0.0.1:'+$server.Port+'/v1",wire_api="responses",requires_openai_auth=false}'
    [IO.File]::WriteAllText((Join-Path $env:AAA_CONFIG_DIR 'host.toml'),@"
schema = 1
[host]
agent_listen = ""
[web]
listen = ""
[local_agent]
mode = "none"
name = "mac-a"
[projects]
root = '$projects'
allow_profiles = ["fixture"]
[safety.probe]
yolo = true
trust_screens = "coordinator"
[profiles.fixture]
harness = "codex"
model = "gpt-5.4"
command = ['$Codex']
args = ['--disable', 'plugins', '--disable', 'remote_plugin', '-c', 'model_provider="fixture"', '-c', '$provider', '-c', 'check_for_update_on_startup=false', '-c', 'analytics.enabled=false']
"@)
    [IO.File]::WriteAllText((Join-Path $env:AAA_CONFIG_DIR 'agent.toml'),@"
schema = 1
[agent]
name = "mac-a"
work_root = '$work'
"@)
    $hostProcess=Start-ProcessOwned $Aaa @('host');Wait-Ready $hostProcess 'host 시작' 'host'
    $agentProcess=Start-ProcessOwned $Aaa @('agent','--host','local','--dev-mode');Wait-Ready $agentProcess 'host에 연결했다' 'agent'
    $hostTail=$hostProcess.StandardError.ReadToEndAsync();$agentTail=$agentProcess.StandardError.ReadToEndAsync()
    $hostOut=$hostProcess.StandardOutput.ReadToEndAsync();$agentOut=$agentProcess.StandardOutput.ReadToEndAsync()
    $null=Invoke-Aaa @('project','create','probe','--thread-profile','fixture','--repo',($origin+'@mac-a'))
    $auth=Join-Path $run 'fake-auth.json'
    $claims='{"sub":"fixture-user","exp":4102444800,"https://api.openai.com/auth":{"chatgpt_account_id":"fixture-account","chatgpt_plan_type":"plus"}}'
    $payload=[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($claims)).TrimEnd('=').Replace('+','-').Replace('/','_')
    $fakeAuth=@{auth_mode='apikey';OPENAI_API_KEY=([IO.File]::ReadAllText((Join-Path $PSScriptRoot 'fixtures/secret-canary.txt')));tokens=@{id_token=('eyJhbGciOiJub25lIn0.'+$payload+'.fixture');access_token=([IO.File]::ReadAllText((Join-Path $PSScriptRoot 'fixtures/secret-canary.txt')));refresh_token=([IO.File]::ReadAllText((Join-Path $PSScriptRoot 'fixtures/secret-canary.txt')));account_id='fixture-account'}}
    [IO.File]::WriteAllText($auth,($fakeAuth | ConvertTo-Json -Depth 4 -Compress))
    $null=Invoke-Aaa @('account','import','fixture','--harness','codex','--file',$auth)
    $task=Join-Path $run 'task.md';[IO.File]::WriteAllText($task,'격리된 seed.txt를 읽고 완료한다.')
    $thread=Invoke-Aaa @('thread','start','probe','--title','onboarding','--profile','fixture','--account','fixture','--machine','mac-a','--kind','worktree','--repo',$origin,'--base','main','--task-file',$task)
    for($session=1;$session -le 2;$session++) {
        if($session -eq 2){$thread=Invoke-Aaa @('thread','restart','probe',$thread.id)}
        if(-not $thread.pane){throw 'thread pane이 없다'}
        $panes.Add($thread.pane)
        if(-not $server.Completed.WaitOne(20000)){throw '실제 Codex 명령 결과 사건 시한 초과'}
        if($server.Error){throw $server.Error}
        $null=Invoke-Aaa @('pane','wait-output',$thread.pane,'--match','AAA_CODEX_SMOKE_OK','--timeout','20000')
        $shown=Invoke-Aaa @('thread','show','probe',$thread.id)
        if($shown.brief -ne 'delivered'){throw '첫 brief가 전달되지 않았다'}
        $configPath=Join-Path $env:AAA_STATE_DIR ('agent\account-runs\probe\'+$thread.id+'\codex-home\config.toml')
        $config=[IO.File]::ReadAllText($configPath)
        $keys=[regex]::Matches($config,'(?m)^\[projects\.("(?:\\.|[^"\\])*")\]$')
        if($keys.Count -ne 1){throw 'cwd 외의 신뢰 키가 있다'}
        $trusted=$keys[0].Groups[1].Value | ConvertFrom-Json
        if($trusted.StartsWith('\\?\') -or (Get-Item -LiteralPath $trusted).FullName -ine (Get-Item -LiteralPath $shown.cwd).FullName){throw '실제 cwd의 DOS 신뢰 키가 아니다'}
        $snapshot=Invoke-Aaa @('pane','read',$thread.pane,'--lines','1000')
        [IO.File]::WriteAllText((Join-Path $run ('screen-'+$session+'.json')),($snapshot | ConvertTo-Json -Depth 10))
        $questions=Invoke-Aaa @('question','list')
        if($questions.questions.Count -ne 0){throw '신뢰·샌드박스·명령 승인 질문이 생겼다'}
        $null=Invoke-Aaa @('pane','kill',$thread.pane)
        # Kill ACK와 host의 종료 반영은 별개다. 종료 상태 사건까지 읽고 재시작한다.
        $exitWatch=[Diagnostics.Stopwatch]::StartNew()
        do {
            $ended=Invoke-Aaa @('pane','get',$thread.pane)
            if($ended.state -ne 'running'){break}
            if($exitWatch.ElapsedMilliseconds -ge 20000){throw 'host pane 종료 반영 사건 시한 초과'}
        } while($true)
    }
    $contexts=@(Get-ChildItem -LiteralPath (Join-Path $env:AAA_STATE_DIR 'agent\account-runs') -Filter '*.jsonl' -Recurse | ForEach-Object {
        foreach($line in [IO.File]::ReadLines($_.FullName)) {
            $item=$line | ConvertFrom-Json
            if($item.type -eq 'turn_context'){$item.payload}
        }
    })
    if($contexts.Count -lt 2){throw '두 실행의 실제 Codex 권한 기록이 없다'}
    foreach($context in $contexts) {
        if($context.approval_policy -ne 'never' -or $context.sandbox_policy.type -ne 'danger-full-access'){throw '실제 Codex 권한이 YOLO와 다르다'}
    }
    if($server.Commands -ne 2){throw '두 실행의 명령 결과가 없다'}
    [ordered]@{result='passed';sessions=2;commands=2;brief='delivered';approval='never';sandbox='danger-full-access';cwd_only=$true} |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $run 'result.json')
    Get-Content -LiteralPath (Join-Path $run 'result.json') -Raw
} catch {
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_ | Out-String))
    foreach($pane in $panes) {
        try {$s=Invoke-Aaa @('pane','read',$pane,'--lines','1000');[IO.File]::WriteAllText((Join-Path $run ('failure-screen-'+$pane+'.json')),($s | ConvertTo-Json -Depth 10))}catch{}
    }
    throw
} finally {
    foreach($pane in $panes){try {$null=Invoke-Aaa @('pane','close',$pane)}catch{}}
    $sessionRoot=Join-Path $env:AAA_STATE_DIR 'agent\sessions'
    if(Test-Path -LiteralPath $sessionRoot) {
        foreach($metaFile in Get-ChildItem -LiteralPath $sessionRoot -Filter meta.json -Recurse) {
            $meta=Get-Content -LiteralPath $metaFile.FullName -Raw | ConvertFrom-Json
            foreach($ownedId in @($meta.child_pid,$meta.host_pid)) {
                if(-not $ownedId){continue}
                $owned=Get-Process -Id $ownedId -ErrorAction SilentlyContinue
                if($owned -and $owned.Path -in @($Aaa,$Codex)){$owned.Kill();$owned.WaitForExit()}
            }
        }
    }
    foreach($p in $processes) {if(-not $p.HasExited){$p.Kill();$p.WaitForExit()}}
    foreach($entry in @(@($hostTail,'host.log'),@($agentTail,'agent.log'))) {
        if($entry[0]){[IO.File]::AppendAllText((Join-Path $run $entry[1]),$entry[0].GetAwaiter().GetResult())}
    }
    $server.Dispose()
}
