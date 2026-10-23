# 별도 Windows SCM 서비스에서 실제 host → thread 준비 → session-host → ConPTY 입출력을 확인한다.
# 예: powershell -File scripts/test-windows-service-pane.ps1 -Aaa C:\aaa-test\t-0000\aaa.exe `
#       -Lab C:\aaa-test\t-0000 -ServiceName aaa-test-t-0000-pane
# Bypass는 시험 자식 PowerShell process에만 적용하며 실행 정책 설정은 쓰지 않는다.
# 기준 목록은 실행자가 저장·대조한다. 기존 서비스·설정·PATH·방화벽은 바꾸지 않는다.
param(
    [Parameter(Mandatory=$true)][string]$Aaa,
    [Parameter(Mandatory=$true)][string]$Lab,
    [Parameter(Mandatory=$true)][ValidatePattern('^aaa-test-[a-z0-9-]+$')][string]$ServiceName,
    [switch]$Worker
)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$Lab=[IO.Path]::GetFullPath($Lab).TrimEnd('\')
$Aaa=[IO.Path]::GetFullPath($Aaa)
if($Lab -notmatch '^C:\\aaa-test\\t-[0-9]+$' -or -not $Aaa.StartsWith($Lab+'\',[StringComparison]::OrdinalIgnoreCase)) {
    throw '전용 C:\aaa-test\t-NNNN 안의 실행 파일만 허용한다'
}
$run=Join-Path $Lab 'service-pane'
if($Worker) {
    try {
        if([Diagnostics.Process]::GetCurrentProcess().SessionId -ne 0){throw 'SCM Session 0이 아니다'}
        if([Security.Principal.WindowsIdentity]::GetCurrent().Name -ine ('NT SERVICE\'+$ServiceName)){throw '시험 가상 서비스 계정이 아니다'}
        $env:AAA_CONFIG_DIR=Join-Path $run 'config'
        $env:AAA_STATE_DIR=Join-Path $run 'state'
        $env:AAA_INSTANCE='aaa-test-'+(Split-Path $Lab -Leaf).Substring(2)+'-p'
        $env:TEMP=$run; $env:TMP=$run
        foreach($key in @('AAA_SESSION_ID','AAA_HOOK_SOCK','AAA_HOOK_TOKEN','AAA_PROJECT','AAA_THREAD')) {
            [Environment]::SetEnvironmentVariable($key,$null,'Process')
        }
        foreach($dir in @($env:AAA_CONFIG_DIR,$env:AAA_STATE_DIR)) {
            [void][IO.Directory]::CreateDirectory($dir)
        }
        # std::fs가 만든 journal 하위 폴더도 token 기본 owner를 받는 실제 경로다.
        $projects=(Join-Path $run 'projects').Replace('\','/')
        $work=(Join-Path $run 'work').Replace('\','/')
        $fixture=Join-Path $run 'harness.ps1'
        [IO.File]::WriteAllText($fixture,@'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
[Console]::WriteLine('AAA_SERVICE_PANE_READY')
while($null -ne ($line=[Console]::ReadLine())) {
    if($line -eq 'aaa-service-ping'){[Console]::WriteLine('AAA_SERVICE_PANE_PONG')}
}
'@,[Text.UTF8Encoding]::new($false))
        $powershell=Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
        $fixtureProfile=@"
[profiles.probe]
harness = "custom"
command = ['$powershell', '-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', '$fixture']
"@
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
allow_profiles = ["probe"]
$fixtureProfile
"@,[Text.UTF8Encoding]::new($false))
        [IO.File]::WriteAllText((Join-Path $env:AAA_CONFIG_DIR 'agent.toml'),@"
schema = 1
[agent]
name = "mac-a"
work_root = '$work'
"@,[Text.UTF8Encoding]::new($false))
        function Remember-Process($Process) {
            $line=$Process.Id.ToString()+"`t"+$Process.StartTime.ToUniversalTime().Ticks.ToString()+"`t"+$Process.Path+"`n"
            [IO.File]::AppendAllText((Join-Path $run 'processes.tsv'),$line,[Text.UTF8Encoding]::new($false))
        }
        function Start-Aaa([string[]]$Arguments) {
            $p=[Diagnostics.Process]::new()
            $p.StartInfo.FileName=$Aaa
            $p.StartInfo.Arguments=($Arguments | ForEach-Object {'"'+$_+'"'}) -join ' '
            $p.StartInfo.UseShellExecute=$false
            $p.StartInfo.CreateNoWindow=$true
            $p.StartInfo.RedirectStandardOutput=$true
            $p.StartInfo.RedirectStandardError=$true
            $p.StartInfo.StandardOutputEncoding=[Text.UTF8Encoding]::new($false)
            $p.StartInfo.StandardErrorEncoding=[Text.UTF8Encoding]::new($false)
            [void]$p.Start()
            Remember-Process $p
            return $p
        }
        function Wait-Log($Process,[string]$Match,[string]$Name) {
            $clock=[Diagnostics.Stopwatch]::StartNew()
            while($true) {
                $line=$Process.StandardError.ReadLineAsync()
                if(-not $line.Wait([int][Math]::Max(1,20000-$clock.ElapsedMilliseconds))){throw "$Name 준비 사건 시한 초과"}
                $text=$line.GetAwaiter().GetResult()
                if($null -eq $text){throw "$Name 준비 전 EOF"}
                [IO.File]::AppendAllText((Join-Path $run ($Name+'.log')),$text+"`n",[Text.UTF8Encoding]::new($false))
                if($text.Contains($Match)){return}
                if($clock.ElapsedMilliseconds -ge 20000){throw "$Name 준비 사건 시한 초과"}
            }
        }
        function Invoke-Aaa([string[]]$Arguments) {
            $p=Start-Aaa (@('--json')+$Arguments)
            $out=$p.StandardOutput.ReadToEndAsync();$err=$p.StandardError.ReadToEndAsync()
            if(-not $p.WaitForExit(30000)){$p.Kill();$p.WaitForExit();throw 'CLI 종료 사건 시한 초과'}
            $text=$out.GetAwaiter().GetResult();$errors=$err.GetAwaiter().GetResult()
            if($p.ExitCode -ne 0){throw ('CLI '+($Arguments -join ' ')+': '+$errors)}
            [IO.File]::AppendAllText((Join-Path $run 'cli.jsonl'),$text+"`n",[Text.UTF8Encoding]::new($false))
            return ($text | ConvertFrom-Json)
        }
        $hostProcess=$null;$agentProcess=$null;$thread=$null
        try {
            $hostProcess=Start-Aaa @('host')
            Wait-Log $hostProcess 'host 시작' 'host'
            $agentProcess=Start-Aaa @('agent','--host','local','--dev-mode')
            Wait-Log $agentProcess 'host에 연결했다' 'agent'
            # 나머지 로그도 비동기로 비워 pipe backpressure를 막는다.
            $hostTail=$hostProcess.StandardError.ReadToEndAsync()
            $agentTail=$agentProcess.StandardError.ReadToEndAsync()
            $hostOut=$hostProcess.StandardOutput.ReadToEndAsync()
            $agentOut=$agentProcess.StandardOutput.ReadToEndAsync()
            $null=Invoke-Aaa @('project','create','probe','--thread-profile','probe')
            $task=Join-Path $run 'task.md'
            [IO.File]::WriteAllText($task,'서비스 pane 입출력 시험',[Text.UTF8Encoding]::new($false))
            $thread=Invoke-Aaa @('thread','start','probe','--title','service-pane','--profile','probe','--machine','mac-a','--kind','dir','--task-file',$task)
            if(-not $thread.pane){throw 'thread pane이 없다'}
            foreach($metaFile in Get-ChildItem -LiteralPath (Join-Path $env:AAA_STATE_DIR 'agent\sessions') -Filter meta.json -Recurse) {
                $meta=Get-Content -LiteralPath $metaFile.FullName -Raw -Encoding UTF8 | ConvertFrom-Json
                foreach($processId in @($meta.host_pid,$meta.child_pid)){Remember-Process (Get-Process -Id $processId)}
            }
            $null=Invoke-Aaa @('pane','wait-output',$thread.pane,'--match','AAA_SERVICE_PANE_READY','--timeout','20000')
            $null=Invoke-Aaa @('pane','run',$thread.pane,'aaa-service-ping')
            $null=Invoke-Aaa @('pane','wait-output',$thread.pane,'--match','AAA_SERVICE_PANE_PONG','--timeout','20000')
            $sessions=@(Get-ChildItem -LiteralPath (Join-Path $env:AAA_STATE_DIR 'agent\sessions') -Directory)
            if($sessions.Count -ne 1){throw '정확히 한 session-host여야 한다'}
            $meta=Get-Content -LiteralPath (Join-Path $sessions[0].FullName 'meta.json') -Raw -Encoding UTF8 | ConvertFrom-Json
            foreach($processId in @($hostProcess.Id,$agentProcess.Id,$meta.host_pid,$meta.child_pid)) {
                $process=Get-Process -Id $processId
                if($process.SessionId -ne 0){throw 'pane 경로가 Session 0 밖이다'}
            }
            [ordered]@{session=0;virtual_service_account=$true;thread=$thread.id;pane=$thread.pane;host_pid=$hostProcess.Id;agent_pid=$agentProcess.Id;session_host_pid=$meta.host_pid;harness_pid=$meta.child_pid;result='pane-ready-and-pong'} |
                ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $run 'result.json') -Encoding UTF8
        } finally {
            if($thread -and $thread.pane){$null=Invoke-Aaa @('pane','close',$thread.pane)}
            # 직접 시작한 host/agent와 해당 시험 session metadata의 PID만 정리한다.
            $sessionRoot=Join-Path $env:AAA_STATE_DIR 'agent\sessions'
            if(Test-Path -LiteralPath $sessionRoot) {
                foreach($metaFile in Get-ChildItem -LiteralPath $sessionRoot -Filter meta.json -Recurse) {
                    $meta=Get-Content -LiteralPath $metaFile.FullName -Raw -Encoding UTF8 | ConvertFrom-Json
                    foreach($processId in @($meta.child_pid,$meta.host_pid)) {
                        $p=Get-Process -Id $processId -ErrorAction SilentlyContinue
                        if($p -and $p.Path -in @($Aaa,$powershell)){$p.Kill();$p.WaitForExit()}
                    }
                }
            }
            foreach($p in @($agentProcess,$hostProcess)) {
                if($p -and -not $p.HasExited){$p.Kill();$p.WaitForExit()}
            }
            if($hostTail){[IO.File]::AppendAllText((Join-Path $run 'host.log'),$hostTail.GetAwaiter().GetResult(),[Text.UTF8Encoding]::new($false))}
            if($agentTail){[IO.File]::AppendAllText((Join-Path $run 'agent.log'),$agentTail.GetAwaiter().GetResult(),[Text.UTF8Encoding]::new($false))}
            # 가상 서비스 계정의 private 객체는 그 계정이 정리한다. 결과·로그는 남긴다.
            foreach($dir in @($env:AAA_STATE_DIR,$env:AAA_CONFIG_DIR,(Join-Path $run 'work'),(Join-Path $run 'projects'))) {
                if(Test-Path -LiteralPath $dir){Remove-Item -LiteralPath $dir -Recurse -Force}
            }
        }
    } catch {
        [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_ | Out-String),[Text.UTF8Encoding]::new($false))
        exit 1
    }
    exit 0
}
if(Test-Path -LiteralPath $run){throw '시험 결과 폴더가 이미 있다'}
if(Get-Service -Name $ServiceName -ErrorAction SilentlyContinue){throw '서비스 이름이 이미 있다'}
[void][IO.Directory]::CreateDirectory($run)
$script=Join-Path $run 'owner.ps1'
Copy-Item -LiteralPath $PSCommandPath -Destination $script
$encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes("& '$script' -Worker -Aaa '$Aaa' -Lab '$Lab' -ServiceName '$ServiceName'"))
$source=@"
using System;
using System.Diagnostics;
using System.ServiceProcess;
public sealed class PaneService : ServiceBase {
 Process worker;
 public PaneService(){ServiceName="$ServiceName";}
 protected override void OnStop(){if(worker!=null && !worker.HasExited){worker.Kill();worker.WaitForExit();}}
 protected override void OnStart(string[] args) {
  System.Threading.ThreadPool.QueueUserWorkItem(_ => {
   try {
    var p=new Process();
    p.StartInfo.FileName=Environment.GetEnvironmentVariable("SystemRoot")+@"\System32\WindowsPowerShell\v1.0\powershell.exe";
    p.StartInfo.Arguments="-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encoded";
    p.StartInfo.UseShellExecute=false;p.StartInfo.CreateNoWindow=true;
    p.StartInfo.RedirectStandardOutput=true;p.StartInfo.RedirectStandardError=true;
    p.Start();worker=p;
    System.IO.File.AppendAllText(@"$run\processes.tsv",p.Id+"\t"+p.StartTime.ToUniversalTime().Ticks+"\t"+p.MainModule.FileName+"\n",new System.Text.UTF8Encoding(false));
    var output=p.StandardOutput.ReadToEndAsync();var error=p.StandardError.ReadToEndAsync();
    p.WaitForExit();ExitCode=p.ExitCode;
    System.IO.File.WriteAllText(@"$run\worker.stdout",output.Result);
    System.IO.File.WriteAllText(@"$run\worker.stderr",error.Result);
   } catch (Exception error) { System.IO.File.WriteAllText(@"$run\wrapper-error.txt",error.ToString()); ExitCode=1; }
   Stop();
  });
 }
 public static void Main(){Run(new PaneService());}
}
"@
$wrapper=Join-Path $run 'service.exe'
Add-Type -TypeDefinition $source -ReferencedAssemblies 'System.ServiceProcess' -OutputAssembly $wrapper -OutputType ConsoleApplication
$created=$false
$labAcl=Get-Acl -LiteralPath $Lab
try {
    # SCM 가상 계정은 실제 암호·사용자 계정 생성 없이 일반 사용자 SID로 실행한다.
    $account='NT SERVICE\'+$ServiceName
    # 가상 계정은 NULL password가 필요하다. PSCredential의 빈 문자열과 구분한다.
    & sc.exe create $ServiceName binPath= $wrapper start= demand obj= $account | Out-Null
    if($LASTEXITCODE -ne 0){throw '가상 계정 시험 서비스 생성 실패'}
    $created=$true
    $sid=[Security.Principal.NTAccount]::new($account).Translate([Security.Principal.SecurityIdentifier])
    $acl=Get-Acl -LiteralPath $Lab
    $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid,'FullControl','ContainerInherit,ObjectInherit','None','Allow'))
    Set-Acl -LiteralPath $Lab -AclObject $acl
    Start-Service -Name $ServiceName
    $service=Get-Service -Name $ServiceName
    $service.WaitForStatus('Stopped',[TimeSpan]::FromSeconds(120))
    if(Test-Path -LiteralPath (Join-Path $run 'failure.txt')){throw [IO.File]::ReadAllText((Join-Path $run 'failure.txt'))}
    if(-not (Test-Path -LiteralPath (Join-Path $run 'result.json'))){throw '서비스 시험 결과가 없다'}
    Get-Content -LiteralPath (Join-Path $run 'result.json') -Raw -Encoding UTF8
} finally {
    if($created) {
        $service=Get-Service -Name $ServiceName
        if($service.Status -ne 'Stopped'){Stop-Service -Name $ServiceName; $service.WaitForStatus('Stopped',[TimeSpan]::FromSeconds(20))}
        & sc.exe delete $ServiceName | Out-Null
        if($LASTEXITCODE -ne 0){throw '시험 서비스 삭제 실패'}
    }
    # 시한 초과에도 직접 관찰한 시험 PID만, 실행 파일·생성 시각까지 대조해 정리한다.
    $records=Join-Path $run 'processes.tsv'
    if(Test-Path -LiteralPath $records) {
        $lines=@([IO.File]::ReadAllLines($records));[Array]::Reverse($lines)
        foreach($line in $lines) {
            $parts=$line -split "`t",3
            $p=Get-Process -Id ([int]$parts[0]) -ErrorAction SilentlyContinue
            if($p -and $p.Path -ieq $parts[2] -and $p.StartTime.ToUniversalTime().Ticks -eq [long]$parts[1]) {
                $p.Kill();$p.WaitForExit()
            }
        }
    }
    Set-Acl -LiteralPath $Lab -AclObject $labAcl
}
