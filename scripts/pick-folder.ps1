param(
  [Parameter(Mandatory=$true)][string]$Folder,
  [int]$TimeoutSec = 20
)
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @"
using System;using System.Text;using System.Runtime.InteropServices;
public class FW {
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
 delegate bool EnumProc(IntPtr h, IntPtr l);
 public static IntPtr FindDialog(uint pid){
   IntPtr found=IntPtr.Zero;
   EnumWindows((h,l)=>{
     if(IsWindowVisible(h)){
       var cn=new StringBuilder(256); GetClassNameW(h,cn,256);
       if(cn.ToString()=="#32770"){
         uint p; GetWindowThreadProcessId(h,out p);
         // Prefer the app's own dialog, but accept any (folder pickers can be
         // hosted by the WebView2 broker process).
         if(found==IntPtr.Zero) found=h;
       }
     }
     return true;
   }, IntPtr.Zero);
   return found;
 }
}
"@
$proc = Get-Process ModernToDoList -ErrorAction SilentlyContinue | Select-Object -First 1
if(-not $proc){ Write-Output "no app process"; exit 1 }

$h = [IntPtr]::Zero
$deadline = (Get-Date).AddSeconds($TimeoutSec)
while((Get-Date) -lt $deadline){
  $h = [FW]::FindDialog([uint32]$proc.Id)
  if($h -ne [IntPtr]::Zero){ break }
  Start-Sleep -Milliseconds 300
}
if($h -eq [IntPtr]::Zero){ Write-Output "dialog not found"; exit 2 }

$win = [System.Windows.Automation.AutomationElement]::FromHandle($h)
$auto = [System.Windows.Automation.AutomationElement]

# set the path edit
$edits = $win.FindAll([System.Windows.Automation.TreeScope]::Descendants, (New-Object System.Windows.Automation.PropertyCondition($auto::ControlTypeProperty, [System.Windows.Automation.ControlType]::Edit)))
$set = $false
for($i=0;$i -lt $edits.Count;$i++){
  $e = $edits.Item($i)
  try{
    $vp = $e.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    $vp.SetValue($Folder); $set=$true; break
  }catch{}
}
if(-not $set){ Write-Output "no editable field set"; exit 3 }

# click the confirm button
$btns = $win.FindAll([System.Windows.Automation.TreeScope]::Descendants, (New-Object System.Windows.Automation.PropertyCondition($auto::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)))
$rx = '选择文件夹|Select Folder|打开|Open|确定|OK|保存|Save'
$done=$false
$target=$null
for($i=0;$i -lt $btns.Count;$i++){
  $b = $btns.Item($i)
  if($b.Current.Name -match $rx){ $target=$b; break }
}
if($target){
  try{ $ip=$target.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern); $ip.Invoke(); $done=$true }catch{}
  if(-not $done){
    # Real mouse click at the button center via SendInput.
    Add-Type @"
using System;using System.Runtime.InteropServices;
public static class MClick{
 [StructLayout(LayoutKind.Sequential)]public struct INPUT{public uint type;public InputUnion u;}
 [StructLayout(LayoutKind.Explicit)]public struct InputUnion{[FieldOffset(0)]public MOUSEINPUT mi;}
 [StructLayout(LayoutKind.Sequential)]public struct MOUSEINPUT{public int dx;public int dy;public uint mouseData;public uint dwFlags;public uint time;public IntPtr dwExtraInfo;}
 [DllImport("user32.dll")]public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")]public static extern void mouse_event(uint f,uint x,uint y,uint d,IntPtr e);
 public static void Click(int x,int y){SetCursorPos(x,y);mouse_event(0x0002,0,0,0,IntPtr.Zero);mouse_event(0x0004,0,0,0,IntPtr.Zero);}
}
"@
    $r=$target.Current.BoundingRectangle
    [MClick]::Click([int]($r.X+$r.Width/2), [int]($r.Y+$r.Height/2))
    $done=$true
    Write-Output "clicked button via SendInput"
  }
}
if(-not $done){
  Add-Type @"
using System;using System.Runtime.InteropServices;
public static class Foc{[DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr h);}
"@
  [Foc]::SetForegroundWindow($h) | Out-Null
  Start-Sleep -Milliseconds 300
  $wsh = New-Object -ComObject WScript.Shell
  if($set){
    # Edit already holds the path via UIAutomation; just confirm.
    $wsh.SendKeys("{ENTER}")
  } else {
    $wsh.SendKeys($Folder.Replace("\","\\").Replace("+","{+}").Replace("^","{^}").Replace("%","{%}"))
    Start-Sleep -Milliseconds 300
    $wsh.SendKeys("{ENTER}")
  }
  $done=$true
  Write-Output "used SendKeys fallback (set=$set)"
}
Write-Output ("folder=$Folder set=$set invoked=$done")
