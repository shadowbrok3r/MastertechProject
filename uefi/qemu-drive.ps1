# Boot a staged ESP under OVMF and drive its TUI over a TCP serial line.
# Usage: drive.ps1 -Dir qemu-app -Keys 'l*14','f' -Settle 12 -Hold 6
param(
  [string]$Dir = 'qemu-app',
  [string[]]$Keys = @(),
  [int]$Settle = 14,
  [int]$Hold = 6,
  [int]$Port = 45454
)

$scratch = $PSScriptRoot
Set-Location $scratch

$qemu = 'C:\Program Files\qemu\qemu-system-x86_64.exe'
$p = Start-Process -FilePath $qemu -ArgumentList @(
  '-machine','q35','-m','768','-display','none','-no-reboot','-net','none',
  '-drive','if=pflash,format=raw,readonly=on,file=code.fd',
  '-drive','if=pflash,format=raw,file=vars.fd',
  '-drive',"file=fat:rw:$Dir,format=raw",
  '-serial',"tcp:127.0.0.1:$Port,server=on,wait=off"
) -NoNewWindow -PassThru

$sb = New-Object System.Text.StringBuilder
try {
  $client = $null
  for ($i = 0; $i -lt 40 -and -not $client; $i++) {
    try { $client = New-Object System.Net.Sockets.TcpClient('127.0.0.1', $Port) } catch { Start-Sleep -Milliseconds 250 }
  }
  if (-not $client) { throw "could not attach to serial on port $Port" }
  $ns = $client.GetStream()
  $buf = New-Object byte[] 65536

  function Pump([int]$seconds) {
    $end = [DateTime]::UtcNow.AddSeconds($seconds)
    while ([DateTime]::UtcNow -lt $end) {
      if ($ns.DataAvailable) {
        $n = $ns.Read($buf, 0, $buf.Length)
        if ($n -gt 0) { [void]$sb.Append([System.Text.Encoding]::ASCII.GetString($buf, 0, $n)) }
      } else { Start-Sleep -Milliseconds 100 }
    }
  }

  Pump $Settle
  foreach ($k in $Keys) {
    # 'x*N' repeats a key N times.
    $text = if ($k -match '^(.)\*(\d+)$') { $Matches[1] * [int]$Matches[2] } else { $k }
    foreach ($ch in $text.ToCharArray()) {
      $b = [System.Text.Encoding]::ASCII.GetBytes([string]$ch)
      $ns.Write($b, 0, $b.Length); $ns.Flush()
      Start-Sleep -Milliseconds 220
      if ($ns.DataAvailable) { $n = $ns.Read($buf, 0, $buf.Length); if ($n -gt 0) { [void]$sb.Append([System.Text.Encoding]::ASCII.GetString($buf, 0, $n)) } }
    }
    Pump 2
  }
  Pump $Hold
} finally {
  if ($client) { $client.Close() }
  if (-not $p.HasExited) { $p.Kill() }
}

$raw = $sb.ToString()
Set-Content "$scratch\lastframe.raw" -Value $raw -Encoding ascii
# Strip ANSI, then keep only the final screen the app painted.
$clean = $raw -replace '\x1b\[[0-9;=]*[A-Za-z]',''
Set-Content "$scratch\lastframe.txt" -Value $clean -Encoding utf8
$clean
