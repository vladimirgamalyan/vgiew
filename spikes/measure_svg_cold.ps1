# Cold-start half of the SVG question: does carrying an SVG renderer slow down the launches
# that are not already in cache?
#
# measure_svg.ps1 covers the warm case, and shows the binary's own first_frame_ms barely
# moves. But first_frame_ms is clocked from the start of `main`, so everything the Windows
# loader and Defender do *before* that is invisible to it — and that is precisely where a
# bigger binary costs something. So this measures from outside, and decomposes the cost,
# because two different size-dependent effects are in play and conflating them would
# badly misattribute the price:
#
#   warm    — repeated runs from the original path. Steady state: same file every day,
#             pages resident, Defender verdict cached. This is the common case.
#   scan    — fresh path, *buffered* copy: pages are in the file cache, but the file is new,
#             so Defender real-time protection scans it. Isolates scan + image mapping.
#   scan+io — fresh path, *unbuffered* copy (FILE_FLAG_NO_BUFFERING): as above, but the
#             pages never entered the cache, so the loader must fault them in from disk.
#
#   (scan+io) - scan  = the demand-paging cost of the extra bytes.
#   scan - warm       = what a *new or just-updated* binary pays before main() runs.
#
# Unbuffered I/O requires sector-aligned writes, so the copy is padded up; trailing bytes
# past the last PE section are ignored by the loader, and the padded copy runs identically.
# Shared DLLs stay warm throughout — the binary under test is the only cold thing.
$ErrorActionPreference = "Stop"

$exeDir = Join-Path $PSScriptRoot "target\release"
$work = Join-Path $env:TEMP "vgiew_svg_cold"
$runs = 8
$SECTOR = 4096
$NO_BUFFERING = 0x20000000
$WRITE_THROUGH = 0x80000000

$targets = @(
    @{ name = "baseline"; exe = Join-Path $exeDir "svg_start_baseline.exe" },
    @{ name = "trim";     exe = Join-Path $exeDir "svg_start_trim.exe" },
    @{ name = "resvg";    exe = Join-Path $exeDir "svg_start_resvg.exe" }
)
foreach ($t in $targets) { if (-not (Test-Path $t.exe)) { throw "not built: $($t.exe)" } }

New-Item -ItemType Directory -Force -Path $work | Out-Null

function Copy-Unbuffered($srcPath, $dstPath) {
    $bytes = [System.IO.File]::ReadAllBytes($srcPath)
    $padded = [math]::Ceiling($bytes.Length / $SECTOR) * $SECTOR
    $buf = New-Object byte[] $padded
    [Array]::Copy($bytes, $buf, $bytes.Length)
    $opts = [System.IO.FileOptions]($NO_BUFFERING -bor $WRITE_THROUGH)
    $fs = New-Object System.IO.FileStream($dstPath, [System.IO.FileMode]::Create,
        [System.IO.FileAccess]::Write, [System.IO.FileShare]::None, $SECTOR, $opts)
    try { $fs.Write($buf, 0, $padded) } finally { $fs.Dispose() }
}

function Invoke-Timed($exe) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $out = & $exe 2>$null
    $sw.Stop()
    $inner = if ($out -match "first_frame_ms=([\d\.]+)") { [double]$Matches[1] } else { [double]::NaN }
    @{ wall = $sw.Elapsed.TotalMilliseconds; inner = $inner }
}

function Mean($arr) { ($arr | Measure-Object -Average).Average }
function Stat($arr) {
    $s = @($arr | Sort-Object)
    "min={0,6:N1}  med={1,6:N1}  mean={2,6:N1}" -f $s[0], $s[[int]([math]::Floor($s.Count / 2))], (Mean $arr)
}

$wall = @{}; $inner = @{}
foreach ($t in $targets) { foreach ($c in @("warm", "scan", "scan+io")) { $wall["$($t.name)/$c"] = @(); $inner["$($t.name)/$c"] = @() } }

# Interleaved across targets so CPU boost / thermal drift hits all of them equally, and a
# fresh path per run so no run can reuse the previous one's mapping or scan verdict.
for ($i = 0; $i -lt $runs; $i++) {
    foreach ($t in $targets) {
        foreach ($c in @("warm", "scan", "scan+io")) {
            $exe = $t.exe
            if ($c -ne "warm") {
                $exe = Join-Path $work "$($t.name)_${c}_$i.exe"
                if ($c -eq "scan") { Copy-Item $t.exe $exe } else { Copy-Unbuffered $t.exe $exe }
            }
            $r = Invoke-Timed $exe
            $wall["$($t.name)/$c"] += $r.wall
            $inner["$($t.name)/$c"] += $r.inner
            # Best-effort: the exited process or Defender may still hold the file briefly.
            # Leftovers are unique per run and the whole work dir is removed at the end.
            if ($c -ne "warm") { try { Remove-Item $exe -Force -ErrorAction Stop } catch {} }
            Start-Sleep -Milliseconds 150
        }
    }
}

Write-Output "=== startup by cache/scan state (wall-clock, n=$runs, DLLs warm throughout) ==="
Write-Output ""
foreach ($t in $targets) {
    "== {0,-8} {1,7:N0} KB ==" -f $t.name, ((Get-Item $t.exe).Length / 1KB)
    foreach ($c in @("warm", "scan", "scan+io")) {
        "   {0,-8} {1}   [inner first_frame mean={2,5:N1}]" -f $c, (Stat $wall["$($t.name)/$c"]), (Mean $inner["$($t.name)/$c"])
    }
}
Write-Output ""
Write-Output "=== delta vs baseline (mean wall-clock, ms) — the price of linking resvg ==="
"{0,-10} {1,10} {2,10} {3,10}" -f "target", "warm", "scan", "scan+io"
foreach ($t in $targets) {
    "{0,-10} {1,10:+0.0;-0.0;0.0} {2,10:+0.0;-0.0;0.0} {3,10:+0.0;-0.0;0.0}" -f $t.name,
        ((Mean $wall["$($t.name)/warm"]) - (Mean $wall["baseline/warm"])),
        ((Mean $wall["$($t.name)/scan"]) - (Mean $wall["baseline/scan"])),
        ((Mean $wall["$($t.name)/scan+io"]) - (Mean $wall["baseline/scan+io"]))
}
Write-Output ""
Write-Output "=== cost decomposition per target (mean wall-clock, ms) ==="
"{0,-10} {1,14} {2,16}" -f "target", "scan-warm", "(scan+io)-scan"
Write-Output "           (new-file cost)  (demand paging)"
foreach ($t in $targets) {
    "{0,-10} {1,14:N1} {2,16:N1}" -f $t.name,
        ((Mean $wall["$($t.name)/scan"]) - (Mean $wall["$($t.name)/warm"])),
        ((Mean $wall["$($t.name)/scan+io"]) - (Mean $wall["$($t.name)/scan"]))
}
Write-Output ""

# Decisive check on *what* the "new file" cost is. Real-time antivirus scans an executable
# the first time it runs at a given identity and then caches the verdict; demand paging, by
# contrast, would be paid again on any later cold launch. So: put down one fresh unbuffered
# copy and run it repeatedly from that same path. If run 1 is expensive and runs 2..n fall
# back to warm levels, the cost is a one-per-build scan, not something every launch pays.
Write-Output "=== same fresh copy, run repeatedly (is the new-file cost one-time?) ==="
"{0,-10} {1,10} {2,10} {3,10}" -f "target", "run1", "run2", "run3..5 mean"
foreach ($t in $targets) {
    $dst = Join-Path $work "repeat_$($t.name).exe"
    Copy-Unbuffered $t.exe $dst
    $seq = @(1..5 | ForEach-Object { (Invoke-Timed $dst).wall; Start-Sleep -Milliseconds 150 })
    "{0,-10} {1,10:N1} {2,10:N1} {3,10:N1}" -f $t.name, $seq[0], $seq[1], (Mean @($seq[2..4]))
    try { Remove-Item $dst -Force -ErrorAction Stop } catch {}
}

try { Remove-Item $work -Recurse -Force -ErrorAction Stop } catch { Write-Output "(leftovers in $work)" }
