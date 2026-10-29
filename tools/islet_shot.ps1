# Starts Islet in demo mode with a fixed state, captures its window and exits.
param(
    [Parameter(Mandatory)][string]$Spec,
    [Parameter(Mandatory)][string]$Out,
    [int]$WaitMs = 1500
)
Get-Process islet -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "D:\islet\*" } | Out-Null
$env:ISLET_DEMO = $Spec
$p = Start-Process -FilePath "D:\islet\target\release\islet.exe" -PassThru
Remove-Item Env:\ISLET_DEMO
Start-Sleep -Milliseconds $WaitMs
$dir = Split-Path $Out
$name = [IO.Path]::GetFileNameWithoutExtension($Out)
& (Join-Path $PSScriptRoot "rec.ps1") -Name "." -Class IsletNotch -Out $dir -Steps "shot:$name" -NoRecord | Out-Null
Stop-Process -Id $p.Id -ErrorAction SilentlyContinue
if (Test-Path $Out) { "saved $Out" } else { "capture failed" }
