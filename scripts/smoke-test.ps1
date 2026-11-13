# End-to-end check of milestone 1 against a running server with an EMPTY database:
#   $env:RGS_DATA_DIR = "test-data"; cargo run    (in one terminal)
#   ./scripts/smoke-test.ps1                      (in another)

$base = "http://127.0.0.1:8650/api/v1"
$ErrorActionPreference = "Stop"

function Call($method, $path, $body, $token) {
    $h = @{}
    if ($token) { $h["Authorization"] = "Bearer $token" }
    try {
        $r = Invoke-RestMethod -Method $method -Uri "$base$path" -Headers $h -ContentType "application/json" -Body ($(if ($body) { $body | ConvertTo-Json } else { $null }))
        return @{ ok = $true; data = $r }
    } catch {
        $msg = $_.ErrorDetails.Message
        return @{ ok = $false; status = [int]$_.Exception.Response.StatusCode; data = $msg }
    }
}

function Totp($base32) {
    $alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"
    $bits = ($base32.ToUpper().TrimEnd('=').ToCharArray() | ForEach-Object { [Convert]::ToString($alphabet.IndexOf($_), 2).PadLeft(5, '0') }) -join ''
    $bytes = for ($i = 0; $i + 8 -le $bits.Length; $i += 8) { [Convert]::ToByte($bits.Substring($i, 8), 2) }
    $counter = [BitConverter]::GetBytes([long][Math]::Floor([DateTimeOffset]::UtcNow.ToUnixTimeSeconds() / 30))
    [Array]::Reverse($counter)
    $hmac = New-Object System.Security.Cryptography.HMACSHA1 (,[byte[]]$bytes)
    $hash = $hmac.ComputeHash($counter)
    $o = $hash[19] -band 0xf
    $num = (([int]$hash[$o] -band 0x7f) -shl 24) -bor ([int]$hash[$o+1] -shl 16) -bor ([int]$hash[$o+2] -shl 8) -bor [int]$hash[$o+3]
    return ($num % 1000000).ToString("D6")
}

function Check($name, $cond) { "{0,-55} {1}" -f $name, $(if ($cond) { "PASS" } else { "FAIL" }) }

$h = Call GET "/health"
Check "health" $h.ok

$owner = Call POST "/auth/register" @{ username = "owner1"; password = "correct horse" }
Check "first account is owner" ($owner.ok -and $owner.data.account.role -eq "owner")
$tok = $owner.data.token
Check "2FA recommended flag on new account" ($owner.data.account.mfa_recommended -eq $true)

$adm = Call POST "/auth/login" @{ username = "owner1"; password = "correct horse"; admin = $true }
Check "admin login refused without 2FA (403)" (-not $adm.ok -and $adm.status -eq 403)

$normal = Call POST "/auth/login" @{ username = "owner1"; password = "correct horse" }
Check "normal login works without 2FA" ($normal.ok -and $normal.data.token)

$bad = Call POST "/auth/login" @{ username = "owner1"; password = "nope nope" }
Check "wrong password refused (401)" (-not $bad.ok -and $bad.status -eq 401)

$setup = Call POST "/auth/2fa/setup" $null $tok
Check "2FA setup gives a secret" ($setup.ok -and $setup.data.secret)
$confirmBad = Call POST "/auth/2fa/confirm" @{ code = "000000" } $tok
Check "wrong 2FA code refused" (-not $confirmBad.ok)
$confirm = Call POST "/auth/2fa/confirm" @{ code = (Totp $setup.data.secret) } $tok
Check "2FA confirm returns 10 recovery codes" ($confirm.ok -and $confirm.data.recovery_codes.Count -eq 10)

$a1 = Call POST "/auth/login" @{ username = "owner1"; password = "correct horse"; admin = $true }
Check "admin login asks for 2FA" ($a1.ok -and $a1.data.mfa_required)
$a2 = Call POST "/auth/login/2fa" @{ ticket = $a1.data.ticket; code = (Totp $setup.data.secret) }
Check "admin login with code" ($a2.ok -and $a2.data.token)
$admin = $a2.data.token

$stats = Call GET "/admin/stats" $null $admin
Check "admin stats with admin session" ($stats.ok -and $stats.data.accounts -eq 1)
$statsWeb = Call GET "/admin/stats" $null $tok
Check "admin API refused with normal session (403)" (-not $statsWeb.ok -and $statsWeb.status -eq 403)

$p = Call POST "/auth/register" @{ username = "player_two"; password = "hunter2hunter2" }
Check "second account is a player" ($p.ok -and $p.data.account.role -eq "player")
$ptok = $p.data.token
$padm = Call GET "/admin/stats" $null $ptok
Check "player can't use admin API" (-not $padm.ok)

# Headset login
$d = Call POST "/device/start" @{ device_name = "Quest 3" }
Check "device start gives a code" ($d.ok -and $d.data.user_code -match '^[A-Z2-9]{3}-[A-Z2-9]{3}$')
$poll1 = Call POST "/device/poll" @{ device_code = $d.data.device_code }
Check "device poll waits before approval" ($poll1.ok -and -not $poll1.data.approved)
$ap = Call POST "/device/approve" @{ user_code = $d.data.user_code.ToLower().Replace("-", "") } $ptok
Check "player approves the code" $ap.ok
$poll2 = Call POST "/device/poll" @{ device_code = $d.data.device_code }
Check "device gets its own token" ($poll2.ok -and $poll2.data.approved -and $poll2.data.token)
$dme = Call GET "/auth/me" $null $poll2.data.token
Check "device token signs in as the player" ($dme.ok -and $dme.data.username -eq "player_two" -and $dme.data.session.kind -eq "Quest 3")
$poll3 = Call POST "/device/poll" @{ device_code = $d.data.device_code }
Check "device code only works once" (-not $poll3.ok)

# Moderation
$list = Call GET "/admin/accounts?q=player" $null $admin
$pid2 = $list.data.accounts[0].id
$role = Call POST "/admin/accounts/$pid2/role" @{ role = "moderator" } $admin
Check "owner makes player a moderator" $role.ok
$ownerRole = Call POST "/admin/accounts/$pid2/role" @{ role = "owner" } $admin
Check "nobody can hand out owner" (-not $ownerRole.ok)
$ban = Call POST "/admin/accounts/$pid2/ban" @{ reason = "testing"; hours = 2 } $admin
Check "owner bans the moderator for 2h" $ban.ok
$banned = Call POST "/auth/login" @{ username = "player_two"; password = "hunter2hunter2" }
Check "banned account can't log in (403)" (-not $banned.ok -and $banned.status -eq 403)
$devAfter = Call GET "/auth/me" $null $poll2.data.token
Check "ban signs out their devices" (-not $devAfter.ok)
$unban = Call POST "/admin/accounts/$pid2/unban" @{} $admin
Check "unban" $unban.ok
$audit = Call GET "/admin/audit" $null $admin
Check "audit log has the role change, ban and unban" ($audit.ok -and $audit.data.entries.Count -ge 3)

$rc = Call POST "/auth/login" @{ username = "owner1"; password = "correct horse" }
$rcl = Call POST "/auth/login/2fa" @{ ticket = $rc.data.ticket; code = $confirm.data.recovery_codes[0] }
Check "recovery code signs in" ($rcl.ok -and $rcl.data.token)
$rc2 = Call POST "/auth/login" @{ username = "owner1"; password = "correct horse" }
$rcl2 = Call POST "/auth/login/2fa" @{ ticket = $rc2.data.ticket; code = $confirm.data.recovery_codes[0] }
Check "recovery code can't be reused" (-not $rcl2.ok)


# Saves, reports and badges (milestone: Lanternfall on RGS)
$p2 = (Call POST "/auth/login" @{ username = "player_two"; password = "hunter2hunter2" }).data.token
$put = Call PUT "/saves/lanternfall/My%20Base" @{ data = '{"ember":5}' } $p2
Check "cloud save stored" $put.ok
$lst = Call GET "/saves/lanternfall" $null $p2
Check "cloud save listed" ($lst.ok -and $lst.data.saves.Count -eq 1 -and $lst.data.saves[0].slot -eq "My Base")
$get = Call GET "/saves/lanternfall/My%20Base" $null $p2
Check "cloud save loads the same data" ($get.ok -and $get.data.data -eq '{"ember":5}')
$other = Call GET "/saves/lanternfall/My%20Base" $null $tok
Check "other players can't read it" (-not $other.ok)
$del = Call DELETE "/saves/lanternfall/My%20Base" $null $p2
Check "cloud save deleted" ($del.ok -and (Call GET "/saves/lanternfall" $null $p2).data.saves.Count -eq 0)

$ownerId = (Call GET "/auth/me" $null $tok).data.id
$badge = Call GET "/players/$ownerId"
Check "owner shows an Owner badge" ($badge.ok -and $badge.data.badge -eq "Owner")
$null = Call POST "/auth/me/settings" @{ show_badge = $false } $tok
Check "owner can hide the badge" ((Call GET "/players/$ownerId").data.badge -eq $null)
$null = Call POST "/auth/me/settings" @{ show_badge = $true } $tok
$p2id = (Call GET "/auth/me" $null $p2).data.id
Check "moderators get a Moderator badge" ((Call GET "/players/$p2id").data.badge -eq "Moderator")
$plain = Call POST "/auth/register" @{ username = "plain_player"; password = "longpassword" }
Check "ordinary players get no badge" ((Call GET "/players/$($plain.data.account.id)").data.badge -eq $null)

$rep = Call POST "/reports" @{ game = "lanternfall"; target_id = $ownerId; target_name = "owner1"; reason = "griefing"; details = "test report"; context = "lobby code TEST" } $p2
Check "player files a report" $rep.ok
$self = Call POST "/reports" @{ game = "lanternfall"; target_id = $p2id; target_name = "player_two"; reason = "other" } $p2
Check "can't report yourself" (-not $self.ok)
$anon = Call POST "/reports" @{ game = "lanternfall"; target_name = "SomeGuest"; reason = "cheating" } $p2
Check "can report someone without an account" $anon.ok
$q = Call GET "/admin/reports" $null $admin
Check "reports show in the admin queue" ($q.ok -and $q.data.open -eq 2)
$h = Call POST "/admin/reports/$($rep.data.id)" @{ status = "dismissed"; note = "test" } $admin
Check "staff dismisses a report" ($h.ok -and (Call GET "/admin/reports" $null $admin).data.open -eq 1)
$pq = Call GET "/admin/reports" $null $p2
Check "players can't see the report queue" (-not $pq.ok)

