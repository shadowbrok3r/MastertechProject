# Joins a device's relay room as role=master, sends JSON commands and prints every reply.
param(
    [string]$DeviceId = "HID-DEV",
    [string]$Relay = "wss://socket.master-tech.app/websocket",
    [string[]]$Commands = @('{"id":"1","cmd":"ping"}', '{"id":"2","cmd":"status"}'),
    [int]$WaitSeconds = 8
)

$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$uri = [Uri]"$($Relay)?room_id=$DeviceId&role=master"
$connect = [Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(20))
$null = $ws.ConnectAsync($uri, $connect.Token).GetAwaiter().GetResult()
"connected: $uri"

foreach ($c in $Commands) {
    $bytes = [Text.Encoding]::UTF8.GetBytes($c)
    $null = $ws.SendAsync([ArraySegment[byte]]::new($bytes), 'Text', $true, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
    ">> $c"
}

# One outstanding receive at a time; cancelling a ClientWebSocket receive aborts the socket.
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
