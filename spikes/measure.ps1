# Cold-start benchmark for the three graphics tiers.
# For each binary: N runs, capturing the internal first_frame_ms (from stdout)
# and the external wall-clock time of the whole process. The first (cold) run is dropped.
$ErrorActionPreference = "Stop"

$exeDir = Join-Path $PSScriptRoot "target\release"
$targets = @("tier_a_eframe", "tier_b_pixels", "tier_c_softbuffer")
$runs = 15

function Stat($arr) {
    $s = $arr | Sort-Object
    $min = $s[0]
    $med = $s[[int]([math]::Floor($s.Count / 2))]
    $mean = ($arr | Measure-Object -Average).Average
    "min={0,6:N1}  med={1,6:N1}  mean={2,6:N1}  (n={3})" -f $min, $med, $mean, $arr.Count
}

foreach ($t in $targets) {
    $exe = Join-Path $exeDir "$t.exe"
    if (-not (Test-Path $exe)) { Write-Output "SKIP $t (not built)"; continue }

    $internal = @()
    $external = @()
    for ($i = 0; $i -lt $runs; $i++) {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $out = & $exe 2>$null
        $sw.Stop()
        $external += $sw.Elapsed.TotalMilliseconds
        if ($out -match "first_frame_ms=([\d\.]+)") { $internal += [double]$Matches[1] }
        Start-Sleep -Milliseconds 100
    }
    # drop the first (cold) run
    $intWarm = @($internal | Select-Object -Skip 1)
    $extWarm = @($external | Select-Object -Skip 1)

    Write-Output "== $t =="
    Write-Output ("  first-frame (internal, ms): " + (Stat $intWarm))
    Write-Output ("  wall-clock  (external, ms): " + (Stat $extWarm))
    Write-Output ""
}
