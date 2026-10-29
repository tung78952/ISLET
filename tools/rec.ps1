# Runs a scripted scenario against a notch window while recording it.
# Steps (";"-separated, coordinates relative to the window):
#   wheel:n  cwheel:n (Ctrl+wheel)  move:x,y  click:x,y  rclick:x,y  down  up  moveabs:x,y  key:0x14  combo:0x5B,0x20  type:text  wait:ms  shot:name
param(
    [Parameter(Mandatory)][string]$Name,
    [string]$Steps = "wait:1000",
    [string]$Class = "NotchWindow",
    [string]$Out = "D:\islet-re\frames",
    [int]$Fps = 40,
    [int]$KeepEvery = 1,
    [switch]$NoRecord
)
Add-Type -AssemblyName System.Drawing
if (-not ("NotchRec" -as [type])) {
    Add-Type -Path (Join-Path $PSScriptRoot "NotchRec.cs") -ReferencedAssemblies System.Drawing
}
[NotchRec]::SetProcessDPIAware() | Out-Null
if (-not [NotchRec]::Find($Class)) { "window $Class not found"; exit 1 }
$dir = Join-Path $Out $Name
New-Item -ItemType Directory -Force $dir | Out-Null
if (-not $NoRecord) { [NotchRec]::Start($Fps) }
foreach ($s in $Steps.Split(";")) {
    $s = $s.Trim(); if (-not $s) { continue }
    $op, $arg = $s.Split(":", 2)
    $n = @(); if ($arg) { $n = $arg.Split(",") }
    switch ($op) {
        "move"    { [NotchRec]::Move([int]$n[0], [int]$n[1]) }
        "moveabs" { [NotchRec]::MoveAbs([int]$n[0], [int]$n[1]) }
        "click"   { [NotchRec]::Click([int]$n[0], [int]$n[1]) }
        "rclick"  { [NotchRec]::RClick([int]$n[0], [int]$n[1]) }
        "down"    { [NotchRec]::Down() }
        "up"      { [NotchRec]::Up() }
        "key"     { [NotchRec]::Key([byte][Convert]::ToInt32($arg, 16)) }
        "combo"   { [NotchRec]::Combo([byte][Convert]::ToInt32($n[0], 16), [byte][Convert]::ToInt32($n[1], 16)) }
        "type"    { [NotchRec]::Type($arg) }
        "wait"    { Start-Sleep -Milliseconds ([int]$arg) }
        "wheel"   { [NotchRec]::Wheel([int]$arg, $false) }
        "cwheel"  { [NotchRec]::Wheel([int]$arg, $true) }
        "shot"    { [NotchRec]::Shot((Join-Path $dir "$arg.png")) }
    }
}
if (-not $NoRecord) {
    $saved = [NotchRec]::Stop($dir, $KeepEvery)
    "recorded $Name : $saved frames -> $dir (window $([NotchRec]::Win.L),$([NotchRec]::Win.T) $([NotchRec]::Win.R - [NotchRec]::Win.L)x$([NotchRec]::Win.B - [NotchRec]::Win.T))"
} else { "done $Name" }
