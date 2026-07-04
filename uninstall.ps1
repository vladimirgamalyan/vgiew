# Removes vgiew associations and deletes the installed folder.
param([string]$InstallDir = "$env:LOCALAPPDATA\Programs\vgiew")
$ErrorActionPreference = "Stop"

Get-Process vgiew -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 200

if (Test-Path "$InstallDir\vgiew.exe") {
    Start-Process -FilePath "$InstallDir\vgiew.exe" -ArgumentList "--unregister" -Wait -NoNewWindow
    Remove-Item -Recurse -Force $InstallDir
    Write-Host "vgiew removed from $InstallDir, associations cleared." -ForegroundColor Green
    Write-Host "If images were set to open with vgiew by default, Windows will prompt for an app again."
} else {
    Write-Host "Not found: $InstallDir (nothing to remove)."
}
