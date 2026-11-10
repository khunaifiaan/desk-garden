# Plays the 18 s demo for the screen recording:
#   morning -> manual switch to evening -> rain -> cursor raking the sand -> "I drank water" splash.
# 1. Make sure desktop icons are visible (right-click desktop > View > Show desktop icons).
# 2. Start recording the full screen (Snipping Tool: Win+Shift+R, or OBS).
# 3. Run:  powershell -ExecutionPolicy Bypass -File scripts\demo.ps1
# The script minimizes all windows, runs the sequence, then restores your windows and Auto modes.
param([string]$Exe = "$PSScriptRoot\..\target\release\DeskGarden.exe")
Add-Type -Namespace M -Name U -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y); [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();'
[void][M.U]::SetProcessDPIAware()
Add-Type -AssemblyName System.Windows.Forms
$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
# Garden centre (design 1000,560 on a 1920x1080 canvas, cover-fitted to the screen)
$s = [math]::Max($b.Width / 1920, $b.Height / 1080)
$ox = ($b.Width - 1920 * $s) / 2; $oy = ($b.Height - 1080 * $s) / 2
function Rake([double]$secs) {
  $t0 = Get-Date
  while (((Get-Date) - $t0).TotalSeconds -lt $secs) {
    $t = ((Get-Date) - $t0).TotalSeconds + $script:phase
    $x = 1000 + 300 * [math]::Sin($t * 1.1); $y = 560 + 130 * [math]::Sin($t * 2.2)
    [void][M.U]::SetCursorPos([int]($b.X + $ox + $x * $s), [int]($b.Y + $oy + $y * $s))
    Start-Sleep -Milliseconds 15
  }
  $script:phase += $secs
}
$script:phase = 0.0

if (-not (Get-Process DeskGarden -ErrorAction SilentlyContinue)) { Start-Process $Exe; Start-Sleep 2 }
& $Exe --time morning --weather clear
$sh = New-Object -ComObject Shell.Application
$sh.MinimizeAll()
[void][M.U]::SetCursorPos($b.Right - 40, $b.Top + 40)
Start-Sleep 4            # let the morning mood settle; start trimming the clip here

Start-Sleep 2            # 0-2 s   morning, gentle sway and falling leaves
Rake 3                   # 2-5 s   cursor rakes the sand
& $Exe --time evening    # 5 s     manual switch to evening (fades over a few seconds)
Rake 4                   # 5-9 s
& $Exe --weather rain    # 9 s     rain rolls in
Rake 4                   # 9-13 s
[void][M.U]::SetCursorPos($b.Right - 40, $b.Top + 40)
Start-Sleep 1
& $Exe --drink           # 14 s    "I drank water": the basin refills with a splash
Start-Sleep 4            # 14-18 s

& $Exe --time auto --weather auto
$sh.UndoMinimizeALL()
"Demo finished. Trim the first ~4 s of the recording."
