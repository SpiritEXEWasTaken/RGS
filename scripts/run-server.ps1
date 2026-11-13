# Keeps RGS running: starts it again after a scheduled restart (or a crash).
#
# To update: put the new build next to the server as rgs-server.new.exe, then schedule a restart
# in the admin panel. Players get the countdown, and when the server stops this script swaps
# the new build in before starting it again. The old build is kept as rgs-server.old.exe.
#
#   ./scripts/run-server.ps1 -Dir C:\RGS
#
# -Dir is the folder with rgs-server.exe and rgs.toml (the server runs from there).

param([string]$Dir = (Get-Location).Path)

$exe = Join-Path $Dir "rgs-server.exe"
$new = Join-Path $Dir "rgs-server.new.exe"
$old = Join-Path $Dir "rgs-server.old.exe"

while ($true) {
    if (Test-Path $new) {
        Write-Host "Installing the new build..."
        if (Test-Path $exe) { Move-Item $exe $old -Force }
        Move-Item $new $exe -Force
    }
    if (-not (Test-Path $exe)) { Write-Host "No rgs-server.exe in $Dir"; exit 1 }

    Write-Host "Starting RGS ($(Get-Date))"
    $p = Start-Process -FilePath $exe -WorkingDirectory $Dir -NoNewWindow -PassThru -Wait
    Write-Host "RGS stopped with code $($p.ExitCode) ($(Get-Date)). Starting again in 3 seconds; Ctrl+C to quit."
    Start-Sleep -Seconds 3
}
