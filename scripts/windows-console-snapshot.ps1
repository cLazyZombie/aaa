# Only the calling test's live child is inspected. No memory dump or input text is written.
param([Parameter(Mandatory=$true)][int]$TargetPid,
      [Parameter(Mandatory=$true)][int]$OwnerPid,
      [Parameter(Mandatory=$true)][string]$ExpectedExe,
      [Parameter(Mandatory=$true)][string]$OutFile)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$phase='verify_child'
try {
    $target=Get-CimInstance Win32_Process -Filter "ProcessId=$TargetPid"
    if(-not $target) {$phase='child_missing';throw 'owned_child_missing'}
    if($target.ParentProcessId -ne $OwnerPid) {$phase='child_owner_mismatch';throw 'owned_child_mismatch'}
    if(-not [string]::Equals($target.ExecutablePath,$ExpectedExe,[StringComparison]::OrdinalIgnoreCase)) {
        $phase='child_image_mismatch'
        throw 'owned_child_mismatch'
    }
    $phase='compile_helper'
    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
public static class ConsoleStackSnapshot {
    public class Frame { public string module; public string offset; public string symbol; }
    public class ThreadView { public int id; public Frame[] frames; public string error; public int os_code; }
    public class View { public int format=1; public int process_id; public ThreadView[] threads; }
    [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenProcess(uint access,bool inherit,int pid);
    [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenThread(uint access,bool inherit,int tid);
    [DllImport("kernel32.dll",SetLastError=true)] static extern int GetProcessIdOfThread(IntPtr thread);
    [DllImport("kernel32.dll",SetLastError=true)] static extern uint SuspendThread(IntPtr thread);
    [DllImport("kernel32.dll",SetLastError=true)] static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetThreadContext(IntPtr thread,IntPtr context);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll",CharSet=CharSet.Unicode,ExactSpelling=true)] static extern IntPtr GetModuleHandleW(string module);
    [DllImport("kernel32.dll",CharSet=CharSet.Ansi,ExactSpelling=true)] static extern IntPtr GetProcAddress(IntPtr module,string name);
    [DllImport("psapi.dll",CharSet=CharSet.Unicode,ExactSpelling=true)] static extern uint GetModuleFileNameExW(IntPtr process,IntPtr module,StringBuilder path,int size);
    [DllImport("dbghelp.dll",SetLastError=true,CharSet=CharSet.Unicode,ExactSpelling=true)] static extern bool SymInitializeW(IntPtr process,string path,bool invade);
    [DllImport("dbghelp.dll")] static extern bool SymCleanup(IntPtr process);
    [DllImport("dbghelp.dll")] static extern uint SymSetOptions(uint options);
    [DllImport("dbghelp.dll")] static extern ulong SymGetModuleBase64(IntPtr process,ulong address);
    [DllImport("dbghelp.dll",SetLastError=true)] static extern bool SymFromAddr(IntPtr process,ulong address,out ulong displacement,IntPtr symbol);
    [DllImport("dbghelp.dll",SetLastError=true)] static extern bool StackWalk64(uint machine,IntPtr process,IntPtr thread,IntPtr frame,IntPtr context,IntPtr read,IntPtr functionTable,IntPtr moduleBase,IntPtr translate);
    static string Clean(string text,int limit) {
        var b=new StringBuilder();
        foreach(char c in text) {
            if(b.Length>=limit)break;
            b.Append((c>='a'&&c<='z')||(c>='A'&&c<='Z')||(c>='0'&&c<='9')||"._:$<>[], &()*+-!".IndexOf(c)>=0?c:'_');
        }
        return b.ToString();
    }
    static Frame Describe(IntPtr process,ulong address,IntPtr symbol) {
        ulong module=SymGetModuleBase64(process,address);
        if(module==0 || address<module)return null;
        var name=new StringBuilder(1024);
        GetModuleFileNameExW(process,new IntPtr(unchecked((long)module)),name,name.Capacity);
        string function="";
        Marshal.WriteInt32(symbol,0,88); // AMD64 SYMBOL_INFO.SizeOfStruct
        Marshal.WriteInt32(symbol,80,512); // MaxNameLen; Name starts at byte 84
        ulong displacement;
        if(SymFromAddr(process,address,out displacement,symbol)) {
            int length=Math.Min(Math.Max(Marshal.ReadInt32(symbol,76),0),256);
            var bytes=new byte[length]; Marshal.Copy(IntPtr.Add(symbol,84),bytes,0,length);
            function=Clean(Encoding.ASCII.GetString(bytes),256);
        }
        return new Frame {module=Clean(Path.GetFileName(name.ToString()),80),offset="0x"+(address-module).ToString("x"),symbol=function};
    }
    static ThreadView ReadThread(IntPtr process,int pid,int tid,IntPtr functionTable,IntPtr moduleBase) {
        var frames=new List<Frame>();var view=new ThreadView{id=tid,error="",frames=new Frame[0]};
        IntPtr thread=OpenThread(0x004a,false,tid); // QUERY_INFORMATION|GET_CONTEXT|SUSPEND_RESUME
        if(thread==IntPtr.Zero){view.error="open_thread";view.os_code=Marshal.GetLastWin32Error();return view;}
        IntPtr raw=Marshal.AllocHGlobal(1232+15),frame=Marshal.AllocHGlobal(512),symbol=Marshal.AllocHGlobal(88+512);
        bool suspended=false;
        try {
            if(GetProcessIdOfThread(thread)!=pid){view.error="thread_owner";return view;}
            if(SuspendThread(thread)==UInt32.MaxValue){view.error="suspend_thread";view.os_code=Marshal.GetLastWin32Error();return view;}
            suspended=true;
            IntPtr context=new IntPtr((raw.ToInt64()+15)&~15L);
            Marshal.Copy(new byte[1232],0,context,1232);Marshal.Copy(new byte[512],0,frame,512);
            Marshal.WriteInt32(context,48,0x0010000b); // AMD64 CONTEXT_FULL
            if(!GetThreadContext(thread,context)){view.error="thread_context";view.os_code=Marshal.GetLastWin32Error();return view;}
            ulong pc=unchecked((ulong)Marshal.ReadInt64(context,248));
            Marshal.WriteInt64(frame,0,unchecked((long)pc));Marshal.WriteInt32(frame,12,3); // AddrPC, AddrModeFlat
            Marshal.WriteInt64(frame,32,Marshal.ReadInt64(context,160));Marshal.WriteInt32(frame,44,3); // RBP
            Marshal.WriteInt64(frame,48,Marshal.ReadInt64(context,152));Marshal.WriteInt32(frame,60,3); // RSP
            Frame first=Describe(process,pc,symbol);if(first!=null)frames.Add(first);
            ulong last=pc;
            for(int n=0;n<32;n++) {
                if(!StackWalk64(0x8664,process,thread,frame,context,IntPtr.Zero,functionTable,moduleBase,IntPtr.Zero))break;
                pc=unchecked((ulong)Marshal.ReadInt64(frame,0));
                if(pc==0)break;if(pc==last){if(n==0)continue;break;}last=pc;
                Frame next=Describe(process,pc,symbol);if(next!=null)frames.Add(next);
            }
            if(frames.Count==0)view.error="empty_stack";
            view.frames=frames.ToArray();return view;
        } finally {
            if(suspended && ResumeThread(thread)==UInt32.MaxValue){view.error="resume_thread";view.os_code=Marshal.GetLastWin32Error();}
            Marshal.FreeHGlobal(symbol);Marshal.FreeHGlobal(frame);Marshal.FreeHGlobal(raw);CloseHandle(thread);
        }
    }
    public static View Capture(int pid,string search) {
        if(IntPtr.Size!=8 || Environment.GetEnvironmentVariable("PROCESSOR_ARCHITECTURE")!="AMD64")throw new NotSupportedException();
        IntPtr process=OpenProcess(0x0410,false,pid); // QUERY_INFORMATION|VM_READ only
        if(process==IntPtr.Zero)throw new System.ComponentModel.Win32Exception();
        bool initialized=false;
        try {
            // Local images/PDB only. No symbol server path, prompt, argument or memory dump.
            SymSetOptions(0x00000004|0x00000002|0x00000200|0x00080000);
            if(!SymInitializeW(process,search,true))throw new System.ComponentModel.Win32Exception();
            initialized=true;
            IntPtr dbg=GetModuleHandleW("dbghelp.dll");
            IntPtr table=GetProcAddress(dbg,"SymFunctionTableAccess64"),module=GetProcAddress(dbg,"SymGetModuleBase64");
            var threads=new List<ThreadView>();
            using(var owner=Process.GetProcessById(pid)) foreach(ProcessThread t in owner.Threads) {
                if(threads.Count>=32)break;
                threads.Add(ReadThread(process,pid,t.Id,table,module));
            }
            return new View {process_id=pid,threads=threads.ToArray()};
        } finally {if(initialized)SymCleanup(process);CloseHandle(process);}
    }
}
'@
    $phase='capture_frames'
    $result=[ConsoleStackSnapshot]::Capture($TargetPid,(Split-Path -Parent $ExpectedExe))
    $result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $OutFile -Encoding UTF8
    exit 0
} catch {
    # Native/API/compile failure is a classification, never arbitrary exception text or a command dump.
    $cause=$_.Exception
    while($cause.InnerException){$cause=$cause.InnerException}
    $osCode=0
    if($cause -is [System.ComponentModel.Win32Exception]){$osCode=$cause.NativeErrorCode}
    [ordered]@{format=1;error='snapshot_failed';phase=$phase;error_type=$cause.GetType().Name;os_code=$osCode} |
        ConvertTo-Json | Set-Content -LiteralPath $OutFile -Encoding UTF8
    exit 1
}
