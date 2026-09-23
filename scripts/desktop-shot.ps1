param(
  [Parameter(Mandatory=$true)][string]$Out,
  [string]$Proc = "ModernToDoList"
)
# Capture the largest visible top-level window of $Proc to a PNG via PrintWindow
# (PW_RENDERFULLCONTENT=2), which works even when the window is not foreground.
Add-Type @"
using System;using System.Text;using System.Runtime.InteropServices;
public class Shot {
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
 [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
 [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
 delegate bool EnumProc(IntPtr h, IntPtr l);
 public static IntPtr Best(uint pid){
   IntPtr best=IntPtr.Zero; long area=0;
   EnumWindows((h,l)=>{
     uint p; GetWindowThreadProcessId(h,out p);
     if(p==pid && IsWindowVisible(h)){
       RECT r; GetWindowRect(h,out r);
       long a=(long)(r.Right-r.Left)*(r.Bottom-r.Top);
       if(a>area){area=a;best=h;}
     }
     return true;
   }, IntPtr.Zero);
   return best;
 }
 public static void Cap(IntPtr h, string outPath){
   RECT r; GetWindowRect(h,out r);
   int w=r.Right-r.Left, hh=r.Bottom-r.Top;
   var bmp=new System.Drawing.Bitmap(w,hh);
   using(var g=System.Drawing.Graphics.FromImage(bmp)){
     var hdc=g.GetHdc();
     PrintWindow(h,hdc,2);
     g.ReleaseHdc(hdc);
   }
   bmp.Save(outPath, System.Drawing.Imaging.ImageFormat.Png);
   bmp.Dispose();
 }
}
"@ -ReferencedAssemblies System.Drawing
$p = Get-Process $Proc -ErrorAction SilentlyContinue | Select-Object -First 1
if(-not $p){ Write-Output "no process $Proc"; exit 1 }
$h = [Shot]::Best([uint32]$p.Id)
if($h -eq [IntPtr]::Zero){ Write-Output "no window"; exit 1 }
Add-Type -AssemblyName System.Drawing
[Shot]::Cap($h, $Out)
Write-Output ("saved " + $Out)
