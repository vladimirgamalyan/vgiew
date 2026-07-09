# Shared Win32 helpers for driving XnView MP without stealing foreground:
#  - Capture via PrintWindow(PW_RENDERFULLCONTENT): renders window content directly.
#  - Input via PostMessage(WM_KEYDOWN/UP): posts keys straight to the HWND.
# Also keeps a best-effort Force-Foreground + SendKeys path as a fallback.
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W32 {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern short VkKeyScan(char c);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@

$global:WM_KEYDOWN = 0x100
$global:WM_KEYUP   = 0x101
$global:WM_CHAR    = 0x102

function Get-Rect([IntPtr]$h) {
  $r = New-Object W32+RECT; [W32]::GetWindowRect($h, [ref]$r) | Out-Null
  [pscustomobject]@{ X=$r.Left; Y=$r.Top; W=($r.Right-$r.Left); H=($r.Bottom-$r.Top) }
}

# Capture a window via PrintWindow — works without foreground / when occluded.
function Shot-PW([IntPtr]$h, [string]$path) {
  $rc = Get-Rect $h
  if ($rc.W -le 0 -or $rc.H -le 0) { return $false }
  $bmp = New-Object System.Drawing.Bitmap $rc.W, $rc.H
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $hdc = $g.GetHdc()
  $ok = [W32]::PrintWindow($h, $hdc, 2)   # PW_RENDERFULLCONTENT
  $g.ReleaseHdc($hdc)
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
  return $ok
}

# Best-effort screen capture of the window region (needs it to be visible/topmost).
function Shot-Screen([IntPtr]$h, [string]$path) {
  $rc = Get-Rect $h
  $bmp = New-Object System.Drawing.Bitmap $rc.W, $rc.H
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($rc.X, $rc.Y, 0, 0, (New-Object System.Drawing.Size($rc.W, $rc.H)))
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
}

function Force-Foreground([IntPtr]$h) {
  if ([W32]::GetForegroundWindow() -eq $h) { return $true }
  [W32]::keybd_event(0x12, 0, 0, [IntPtr]::Zero)   # ALT down (lifts foreground lock)
  [W32]::keybd_event(0x12, 0, 2, [IntPtr]::Zero)   # ALT up
  [W32]::ShowWindow($h, 9) | Out-Null              # SW_RESTORE
  $fg = [W32]::GetForegroundWindow()
  $fgT = [W32]::GetWindowThreadProcessId($fg, [IntPtr]::Zero)
  $myT = [W32]::GetCurrentThreadId()
  [W32]::AttachThreadInput($fgT, $myT, $true) | Out-Null
  [W32]::BringWindowToTop($h) | Out-Null
  [W32]::SetForegroundWindow($h) | Out-Null
  [W32]::AttachThreadInput($fgT, $myT, $false) | Out-Null
  Start-Sleep -Milliseconds 250
  return ([W32]::GetForegroundWindow() -eq $h)
}

# Post a virtual-key press+release straight to the window (no foreground needed).
function Post-Vk([IntPtr]$h, [int]$vk) {
  [W32]::PostMessage($h, $global:WM_KEYDOWN, [IntPtr]$vk, [IntPtr]0) | Out-Null
  Start-Sleep -Milliseconds 40
  [W32]::PostMessage($h, $global:WM_KEYUP, [IntPtr]$vk, [IntPtr]0) | Out-Null
  Start-Sleep -Milliseconds 450
}

$global:XNEXE = "C:\Program Files\XnViewMP\xnviewmp.exe"

# Close any XnView, relaunch on $img, return the main window HWND once ready.
function Open-Fresh([string]$img) {
  Get-Process xnviewmp -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
  Start-Sleep -Milliseconds 700
  Get-Process xnviewmp -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 500
  $p = Start-Process $global:XNEXE -ArgumentList "`"$img`"" -PassThru
  for ($i=0; $i -lt 50; $i++) { Start-Sleep -Milliseconds 300; $p.Refresh(); if ($p.MainWindowHandle -ne 0) { break } }
  Start-Sleep -Milliseconds 1600
  return (Get-Process xnviewmp | Select-Object -First 1).MainWindowHandle
}

# VK constants used by the tour.
$global:VK_NEXT=0x22; $global:VK_PRIOR=0x21; $global:VK_ADD=0x6B; $global:VK_SUB=0x6D
