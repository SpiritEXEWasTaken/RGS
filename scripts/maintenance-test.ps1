# Checks restarts, maintenance and version checks. Needs a server with an EMPTY database, run
# through scripts/run-server.ps1 (it gets restarted), with this in its rgs.toml:
#   restart_grace_seconds = 5
#   [[games]]
#   id = "lanternfall"
#   name = "Lanternfall"
#   latest_version = "0.3"
#   min_version = "0.3"

$base = "http://127.0.0.1:8650/api/v1"
$src = Get-Content (Join-Path $PSScriptRoot "smoke-test.ps1") -Raw
Invoke-Expression ([regex]::Match($src, '(?s)function Call.*?\n}\r?\n\r?\nfunction Totp.*?\n}\r?\n\r?\nfunction Check.*?\n').Value)

function CallH($method, $path, $body, $token, $headers) {
    $h = @{}; if ($token) { $h["Authorization"] = "Bearer $token" }; foreach ($k in $headers.Keys) { $h[$k] = $headers[$k] }
    try { @{ ok = $true; data = (Invoke-RestMethod -Method $method -Uri "$base$path" -Headers $h -ContentType "application/json" -Body ($(if ($body) { $body | ConvertTo-Json } else { $null }))) } }
    catch { @{ ok = $false; status = [int]$_.Exception.Response.StatusCode; data = ($_.ErrorDetails.Message | ConvertFrom-Json) } }
}

$o = Call POST "/auth/register" @{ username = "boss"; password = "longpassword" }
$s = Call POST "/auth/2fa/setup" $null $o.data.token
$null = Call POST "/auth/2fa/confirm" @{ code = (Totp $s.data.secret) } $o.data.token
function AdminToken { $a = Call POST "/auth/login" @{ username = "boss"; password = "longpassword"; admin = $true }; (Call POST "/auth/login/2fa" @{ ticket = $a.data.ticket; code = (Totp $s.data.secret) }).data.token }
$admin = AdminToken
$p = Call POST "/auth/register" @{ username = "kid"; password = "longpassword" }

$st = Call GET "/status"
Check "status online, lists Lanternfall" ($st.data.status -eq "online" -and $st.data.games[0].min_version -eq "0.3")

$old = CallH GET "/auth/me" $null $p.data.token @{ "X-Game-Id" = "lanternfall"; "X-Game-Version" = "0.2" }
Check "old game gets update_required (426)" (-not $old.ok -and $old.status -eq 426 -and $old.data.error -eq "update_required")
"    Error: " + $old.data.message
$new = CallH GET "/auth/me" $null $p.data.token @{ "X-Game-Id" = "lanternfall"; "X-Game-Version" = "0.3" }
Check "current game is let in" $new.ok
$newer = CallH GET "/auth/me" $null $p.data.token @{ "X-Game-Id" = "lanternfall"; "X-Game-Version" = "0.10" }
Check "0.10 counts as newer than 0.3" $newer.ok

$m = Call POST "/admin/maintenance" @{ in_minutes = 45; reason = "Update 0.3"; restart = $true } $admin
$st = Call GET "/status"
Check "45 min restart: countdown step 45" ($st.data.status -eq "maintenance_scheduled" -and $st.data.maintenance.announcement.step -eq 45)
"    " + $st.data.maintenance.announcement.text
$pl = Call POST "/auth/login" @{ username = "kid"; password = "longpassword" }
Check "players can still log in before it starts" $pl.ok
$null = Call POST "/admin/maintenance" @{ in_minutes = 3; reason = ""; restart = $true } $admin
$st = Call GET "/status"
Check "3 min out: step 5 announcement" ($st.data.maintenance.announcement.step -eq 5)
"    " + $st.data.maintenance.announcement.text
$c = Call DELETE "/admin/maintenance" $null $admin
Check "cancel brings it back online" ((Call GET "/status").data.status -eq "online")

$null = Call POST "/admin/maintenance" @{ in_minutes = 0; reason = ""; restart = $true } $admin
$st = Call GET "/status"
Check "restart now: status maintenance, step 0" ($st.data.status -eq "maintenance" -and $st.data.maintenance.announcement.step -eq 0)
$pl = CallH POST "/auth/login" @{ username = "kid"; password = "longpassword" } $null @{}
Check "player login refused with maintenance (503)" (-not $pl.ok -and $pl.status -eq 503 -and $pl.data.error -eq "maintenance")
"    Error: " + $pl.data.message
$reg = CallH POST "/auth/register" @{ username = "late"; password = "longpassword" } $null @{}
Check "sign-ups refused too" (-not $reg.ok -and $reg.data.error -eq "maintenance")
$staff = AdminToken
Check "staff can still sign in to the admin panel" ([bool]$staff)

# The server stops after the grace period and the run script starts it again.
$down = $false; for ($i = 0; $i -lt 40 -and -not $down; $i++) { Start-Sleep 1; try { Invoke-RestMethod "$base/health" | Out-Null } catch { $down = $true } }
Check "server stopped for the restart" $down
$up = $false; for ($i = 0; $i -lt 60 -and -not $up; $i++) { Start-Sleep 1; try { Invoke-RestMethod "$base/health" | Out-Null; $up = $true } catch {} }
Check "run script brought it back" $up
$st = Call GET "/status"
Check "back online after the restart" ($st.data.status -eq "online")
$pl = Call POST "/auth/login" @{ username = "kid"; password = "longpassword" }
Check "players can log in again" $pl.ok

$null = Call POST "/admin/maintenance" @{ in_minutes = 0; reason = "database move"; restart = $false } (AdminToken)
$pl = CallH POST "/auth/login" @{ username = "kid"; password = "longpassword" } $null @{}
"    Error: " + $pl.data.message
Check "maintenance (no restart) keeps players out" ($pl.data.error -eq "maintenance")
