# What SVG support would cost at startup.
#
# Part 1 — the price of *linking* an SVG renderer. `svg_start_baseline` and
# `svg_start_resvg` have an identical startup path; the only difference is that the
# second one carries resvg/usvg/tiny-skia. Runs are interleaved A,B,A,B,... rather than
# block-sequential, so CPU boost/thermal drift over the measurement hits both binaries
# equally instead of favouring whichever went first. The first run of each is reported
# separately (cold-ish) and dropped from the warm statistics.
#
# Part 2 — the price of *using* it: the per-file breakdown a double-clicked .svg pays
# before its first pixels appear, plus re-render cost at 4x/16x zoom (the per-gesture
# cost of viewport-only rasterization), and system font loading on its own.
$ErrorActionPreference = "Stop"

$exeDir = Join-Path $PSScriptRoot "target\release"
$svgDir = Join-Path $PSScriptRoot "..\test-images\svg"
$runs = 15

$baseline = Join-Path $exeDir "svg_start_baseline.exe"
$withSvg = Join-Path $exeDir "svg_start_resvg.exe"

function Stat($arr) {
    $s = @($arr | Sort-Object)
    $min = $s[0]
    $med = $s[[int]([math]::Floor($s.Count / 2))]
    $mean = ($arr | Measure-Object -Average).Average
    "min={0,6:N1}  med={1,6:N1}  mean={2,6:N1}  (n={3})" -f $min, $med, $mean, $arr.Count
}

foreach ($p in @($baseline, $withSvg)) {
    if (-not (Test-Path $p)) { throw "not built: $p  (cargo build --release)" }
}

Write-Output "=== binary size ==="
foreach ($p in @($baseline, $withSvg)) {
    "{0,-24} {1,8:N0} KB" -f (Split-Path $p -Leaf), ((Get-Item $p).Length / 1KB)
}
$delta = ((Get-Item $withSvg).Length - (Get-Item $baseline).Length) / 1KB
"{0,-24} {1,8:N0} KB" -f "delta (resvg)", $delta
Write-Output ""

# ── Part 1: startup, interleaved ──
$res = @{ baseline = @(); resvg = @() }
$ext = @{ baseline = @(); resvg = @() }
for ($i = 0; $i -lt $runs; $i++) {
    foreach ($pair in @(@("baseline", $baseline), @("resvg", $withSvg))) {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $out = & $pair[1] 2>$null
        $sw.Stop()
        $ext[$pair[0]] += $sw.Elapsed.TotalMilliseconds
        if ($out -match "first_frame_ms=([\d\.]+)") { $res[$pair[0]] += [double]$Matches[1] }
        Start-Sleep -Milliseconds 100
    }
}

Write-Output "=== startup: winit+softbuffer, with vs without resvg linked ==="
foreach ($k in @("baseline", "resvg")) {
    Write-Output "== $k =="
    Write-Output ("  run 1 (cold-ish):            first_frame={0:N1} ms  wall={1:N1} ms" -f $res[$k][0], $ext[$k][0])
    Write-Output ("  first-frame (internal, ms):  " + (Stat @($res[$k] | Select-Object -Skip 1)))
    Write-Output ("  wall-clock  (external, ms):  " + (Stat @($ext[$k] | Select-Object -Skip 1)))
}
$mBase = (@($res.baseline | Select-Object -Skip 1) | Measure-Object -Average).Average
$mSvg = (@($res.resvg | Select-Object -Skip 1) | Measure-Object -Average).Average
Write-Output ("  => first-frame delta from linking resvg: {0:+0.00;-0.00;0.00} ms" -f ($mSvg - $mBase))
Write-Output ""

# ── Part 2: what rendering an SVG actually costs ──
Write-Output "=== system font loading, on its own ==="
& $withSvg --fonts
Write-Output ""

Write-Output "=== per-file cost (viewport 1600x1000) ==="
Get-ChildItem (Join-Path $svgDir "*.svg") | Sort-Object Length | ForEach-Object {
    & $withSvg --bench $_.FullName 1600 1000
}
