Add-Type @"
using System;using System.Text;using System.Runtime.InteropServices;using System.Collections.Generic;
public class WinEnum {
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
 [DllImport("user32.dll")] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
 [DllImport("user32.dll")] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
 delegate bool EnumProc(IntPtr h, IntPtr l);
 public static List<string> Find(uint pid){
   var r=new List<string>();
   EnumWindows((h,l)=>{
     if(IsWindowVisible(h)){
       uint p; GetWindowThreadProcessId(h,out p);
       if(p==pid){
         var sb=new StringBuilder(256); GetWindowTextW(h,sb,256);
         var cn=new StringBuilder(256); GetClassNameW(h,cn,256);
         r.Add(cn.ToString()+" | "+sb.ToString());
       }
     }
     return true;
   }, IntPtr.Zero);
   return r;
 }
}
"@
$procs = Get-Process ModernToDoList -ErrorAction SilentlyContinue
if (-not $procs) { Write-Output "no ModernToDoList process running" }
foreach($p in $procs){
  Write-Output ("PID " + $p.Id)
  [WinEnum]::Find([uint32]$p.Id) | ForEach-Object { Write-Output ("  win: " + $_) }
}
