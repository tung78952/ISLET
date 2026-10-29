# Captures Islet in each demo state and diffs it against the matching reference frame.
param([string]$Only = "")
$F = "D:\islet-re\frames"; $Out = "D:\islet-re\cmp"; $BG = "$F\01_hover\0233.png"
$song = "title=siopaolo - better for you [visualizer];artist=siopaolo;art=D:\islet-re\art.png;dur=290;playing=1;charging=1"
$box = "138,0,662,175,35"
$cases = @(
    @{ n = "music";    ref = "03_pages\p0";        spec = "page=music;expanded=1;clock=20:06;pos=100;$song"; box = $box },
    @{ n = "pomo";     ref = "03_pages\p1";        spec = "page=pomo;expanded=1;clock=20:06;pos=100;$song"; box = $box },
    @{ n = "water";    ref = "03_pages\p2";        spec = "page=water;expanded=1;clock=20:06;pos=100;$song"; box = $box },
    @{ n = "shelf";    ref = "03_pages\p3";        spec = "page=shelf;expanded=1;clock=20:06;pos=100;$song"; box = $box },
    @{ n = "todo";     ref = "03_pages\p4";        spec = "page=todo;expanded=1;clock=20:06;pos=100;$song"; box = $box },
    @{ n = "settings"; ref = "07_water\e_settings"; spec = "page=water;watersettings=1;expanded=1;clock=20:07;pos=100;$song"; box = $box },
    @{ n = "capson";   ref = "11_rest\d_caps_on";  spec = "caps=1;clock=20:05;pos=100;$song"; box = "200,0,600,46,16" },
    @{ n = "capsoff";  ref = "11_rest\e_caps_off"; spec = "caps=0;clock=20:05;pos=100;$song"; box = "200,0,600,46,16" },
    @{ n = "volume";   ref = "11_rest\f_vol_up";   spec = "volume=0.6;clock=20:05;pos=100;$song"; box = "200,0,600,46,16" },
    @{ n = "compact";  ref = "02_rest\b";          spec = "clock=20:05;pos=100;$song"; box = "307,0,493,37,13" }
)
foreach ($c in $cases) {
    if ($Only -and $c.n -ne $Only) { continue }
    & "$PSScriptRoot\islet_shot.ps1" -Spec $c.spec -Out "$Out\$($c.n).png" | Out-Null
    $r = python -I "$PSScriptRoot\diff.py" $BG "$F\$($c.ref).png" "$Out\$($c.n).png" "$Out\$($c.n)" $c.box
    "{0,-9} {1}" -f $c.n, $r
}
