# Samples DeskGarden.exe for N seconds and prints the numbers MEASUREMENTS.md asks for.
# Usage: powershell -File scripts\measure.ps1 [-Seconds 30]
param([int]$Seconds = 30)
$p = Get-Process DeskGarden -ErrorAction Stop | Select-Object -First 1
$id = $p.Id
$cores = [Environment]::ProcessorCount
$inst = (Get-Counter "\Process(DeskGarden*)\ID Process").CounterSamples | Where-Object { $_.CookedValue -eq $id } | Select-Object -First 1
$name = $inst.InstanceName
$cpu = @(); $ws = @(); $gpu = @()
$cpu0 = (Get-Process -Id $id).TotalProcessorTime; $wall0 = Get-Date
for ($i = 0; $i -lt $Seconds; $i++) {
  $s = Get-Counter "\Process($name)\% Processor Time", "\Process($name)\Working Set - Private" -SampleInterval 1 -MaxSamples 1
  $cpu += ($s.CounterSamples[0].CookedValue / $cores)
  $ws  += ($s.CounterSamples[1].CookedValue / 1MB)
  $g = (Get-Counter "\GPU Engine(pid_$($id)_*)\Utilization Percentage" -ErrorAction SilentlyContinue).CounterSamples
  $gpu += (($g | Measure-Object CookedValue -Sum).Sum)
}
$cpuTotal = ((Get-Process -Id $id).TotalProcessorTime - $cpu0).TotalSeconds / ((Get-Date) - $wall0).TotalSeconds / $cores * 100
"{0,-28} {1,8:N1} MB (max {2:N1})" -f "Private working set (avg)", ($ws | Measure-Object -Average).Average, ($ws | Measure-Object -Maximum).Maximum
"{0,-28} {1,8:N2} %  (max {2:N2})" -f "CPU, all cores (avg)", ($cpu | Measure-Object -Average).Average, ($cpu | Measure-Object -Maximum).Maximum
"{0,-28} {1,8:N3} %" -f "CPU, process time / wall", $cpuTotal
"{0,-28} {1,8:N2} %  (max {2:N2})" -f "GPU engines (avg)", ($gpu | Measure-Object -Average).Average, ($gpu | Measure-Object -Maximum).Maximum
$log = Join-Path $env:APPDATA "KHYR\DeskGarden\deskgarden.log"
if (Test-Path $log) { Select-String -Path $log -Pattern "first frame" | Select-Object -Last 1 | ForEach-Object { "Startup: $($_.Line)" } }
