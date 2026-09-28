# Friendly CLI for the bench USB HID injector over the relay. Builds the JSON so no
# hand-quoting is needed (avoids the `pwsh -File` array-splat trap).
#
#   pwsh -File hid.ps1 -Status
#   pwsh -File hid.ps1 -Type "hello world"
#   pwsh -File hid.ps1 -Key ctrl+alt+del
#   pwsh -File hid.ps1 -Click right
#   pwsh -File hid.ps1 -MoveX 16383 -MoveY 16383
#   pwsh -File hid.ps1 -Arm        # leave armed
#   pwsh -File hid.ps1 -Disarm
# Injecting actions (Type/Key/Click/Move) auto arm before and disarm after unless -StayArmed.
param(
    [string]$DeviceId = "HID-DEV",
    [string]$Relay = "wss://socket.master-tech.app/websocket",
    [string]$Type,
    [string]$Key,
    [string]$Click,
    [int]$MoveX = -1,
    [int]$MoveY = -1,
    [switch]$Status,
    [switch]$Arm,
    [switch]$Disarm,
    [switch]$Release,
    [switch]$StayArmed,
    [int]$WaitSeconds = 12
)

function Cmd($obj) { $obj | ConvertTo-Json -Compress }

$actions = @()
if ($Type)  { $actions += (Cmd @{ cmd = "type";  text = $Type }) }
if ($Key)   { $actions += (Cmd @{ cmd = "key";   chord = $Key }) }
if ($Click) { $actions += (Cmd @{ cmd = "click"; button = $Click }) }
if ($MoveX -ge 0 -and $MoveY -ge 0) { $actions += (Cmd @{ cmd = "mouse_move"; x = $MoveX; y = $MoveY }) }

$cmds = @()
if ($actions.Count -gt 0) {
    $cmds += (Cmd @{ cmd = "arm" })
    $cmds += $actions
    if (-not $StayArmed) { $cmds += (Cmd @{ cmd = "disarm" }) }
}
if ($Arm)     { $cmds += (Cmd @{ cmd = "arm" }) }
if ($Disarm)  { $cmds += (Cmd @{ cmd = "disarm" }) }
if ($Release) { $cmds += (Cmd @{ cmd = "release_all" }) }
if ($Status -or $cmds.Count -eq 0) { $cmds += (Cmd @{ cmd = "status" }) }

$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$uri = [Uri]"$($Relay)?room_id=$DeviceId&role=master"
$cts = [Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(20))
$null = $ws.ConnectAsync($uri, $cts.Token).GetAwaiter().GetResult()
"connected: $DeviceId"

foreach ($c in $cmds) {
    $bytes = [Text.Encoding]::UTF8.GetBytes($c)
    $null = $ws.SendAsync([ArraySegment[byte]]::new($bytes), 'Text', $true, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
    ">> $c"
}

$buf = [byte[]]::new(16384)
$acc = ""
$pending = $null
$deadline = (Get-Date).AddSeconds($WaitSeconds)
while ((Get-Date) -lt $deadline -and $ws.State -eq 'Open') {
    if (-not $pending) {
        $pending = $ws.ReceiveAsync([ArraySegment[byte]]::new($buf), [Threading.CancellationToken]::None)
    }
    if ($pending.Wait(500)) {
        $r = $pending.Result
        $pending = $null
        if ($r.MessageType -eq 'Close') { "<< [close]"; break }
        $acc += [Text.Encoding]::UTF8.GetString($buf, 0, $r.Count)
        if ($r.EndOfMessage) { "<< $acc"; $acc = "" }
    }
}
$ws.Abort()
