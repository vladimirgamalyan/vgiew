# Benchmark of the tuned GPU path (wgpu 22) in different configurations.
# For each (power, backend): N runs, print the chosen adapter and ms to the first frame.
$ErrorActionPreference = "Stop"

$exe = Join-Path $PSScriptRoot "target\release\gpu_tuned.exe"
if (-not (Test-Path $exe)) { Write-Output "gpu_tuned not built"; exit 1 }

$configs = @(
    @("low", "all"),
    @("low", "dx12"),
    @("low", "vulkan"),
    @("low", "gl"),
    @("high", "dx12")
)
$runs = 8

foreach ($c in $configs) {
    $power = $c[0]; $backend = $c[1]
    $internal = @()
    $adapter = ""
    for ($i = 0; $i -lt $runs; $i++) {
        $out = & $exe $power $backend 2>$null
        foreach ($line in $out) {
            if ($line -match "first_frame_ms=([\d\.]+)") { $internal += [double]$Matches[1] }
            if ($line -match "^config:") { $adapter = $line }
        }
        Start-Sleep -Milliseconds 120
    }
    $warm = @($internal | Select-Object -Skip 1)
    if ($warm.Count -eq 0) { Write-Output "[$power/$backend] no data (backend may be unavailable)"; continue }
    $s = $warm | Sort-Object
    $min = $s[0]; $med = $s[[int]([math]::Floor($s.Count / 2))]
    $mean = ($warm | Measure-Object -Average).Average
    Write-Output ("[{0,-4}/{1,-6}] first_frame: min={2,7:N1}  med={3,7:N1}  mean={4,7:N1} ms  (n={5})" -f $power, $backend, $min, $med, $mean, $warm.Count)
    if ($adapter) { Write-Output ("           {0}" -f ($adapter -replace '^config:\s*', '')) }
}
