//! Moves a customer's data off an old Windows drive: QuickBooks company files and browser bookmarks and
//! history, never extensions or settings.

use facet::Facet;
use mtech_plugin_sdk::{SdkError, host, mtech_plugin};
use serde::Deserialize;

const HELPERS_PS: &str = r##"
$ErrorActionPreference = 'SilentlyContinue'
$sys = $env:SystemDrive.TrimEnd('\') + '\'
$skipProfiles = @('Default', 'Default User', 'All Users', 'Public', 'defaultuser0', 'WDAGUtilityAccount')
$chromium = [ordered]@{
    chrome = @('AppData\Local\Google\Chrome\User Data', 'chrome')
    edge = @('AppData\Local\Microsoft\Edge\User Data', 'msedge')
    brave = @('AppData\Local\BraveSoftware\Brave-Browser\User Data', 'brave')
}
$qbFilters = @('*.qbw', '*.qbb', '*.qbm', '*.qbx')
$qbExcludes = @('Windows', '$Recycle.Bin', 'System Volume Information', 'Program Files', 'Program Files (x86)', 'ProgramData', 'AppData', 'Sample Company Files', 'QuickBooksAutoDataRecovery', 'Recovery', '$WinREAgent', 'MSOCache', 'PerfLogs')
function MB($b) { [math]::Round([double]$b / 1MB, 1) }
function Day-Of([string]$path) { $i = Get-Item -LiteralPath $path -Force; if ($i) { $i.LastWriteTime.ToString('yyyy-MM-dd') } else { '' } }
function Size-Of([string]$path) { $i = Get-Item -LiteralPath $path -Force; if ($i) { [int64]$i.Length } else { [int64]0 } }
function Read-Json([string]$path) {
    if (-not (Test-Path -LiteralPath $path)) { return $null }
    [string](Get-Content -LiteralPath $path -Raw -Encoding UTF8) | ConvertFrom-Json
}
function Count-Urls($node) {
    if ($null -eq $node) { return 0 }
    if ($node.type -eq 'url') { return 1 }
    $n = 0
    foreach ($c in @($node.children)) { $n += Count-Urls $c }
    $n
}
function Bookmark-Count([string]$file) {
    $j = Read-Json $file
    if (-not $j) { return 0 }
    (Count-Urls $j.roots.bookmark_bar) + (Count-Urls $j.roots.other) + (Count-Urls $j.roots.synced)
}
function Ext-Name([string]$manifest) {
    $m = Read-Json $manifest
    $name = [string]$m.name
    if ($name -like '__MSG_*') {
        $key = $name.Substring(6).TrimEnd('_')
        $dir = Split-Path $manifest
        foreach ($loc in @('en', 'en_US', [string]$m.default_locale)) {
            $msgs = Join-Path $dir "_locales\$loc\messages.json"
            if ($loc -and (Test-Path -LiteralPath $msgs)) {
                $j = Read-Json $msgs
                $hit = $j.PSObject.Properties | Where-Object { $_.Name -ieq $key } | Select-Object -First 1
                if ($hit) { return [string]$hit.Value.message }
            }
        }
    }
    $name
}
function Chromium-Extensions([string]$profileDir) {
    @(Get-ChildItem -Path (Join-Path $profileDir 'Extensions\*\*\manifest.json') -Force | ForEach-Object { Ext-Name $_.FullName } | Where-Object { $_ } | Sort-Object -Unique)
}
function Firefox-Extensions([string]$profileDir) {
    $j = Read-Json (Join-Path $profileDir 'extensions.json')
    @(@($j.addons) | Where-Object { $_.type -eq 'extension' -and $_.location -eq 'app-profile' } | ForEach-Object { [string]$_.defaultLocale.name } | Where-Object { $_ } | Sort-Object -Unique)
}
function Saved-Logins([string]$file) {
    if (-not (Test-Path -LiteralPath $file)) { return 0 }
    $text = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($file))
    ([regex]::Matches($text, 'https?://[\x21-\x7e]{4,}')).Count
}
# Lists files matching $filters under $root with robocopy /L: full path, size and modified time.
function Find-Files([string]$root, [string[]]$filters, [string[]]$excludeDirs) {
    $null = New-Item -ItemType Directory -Path "$env:TEMP\mtech-migrate-null" -Force
    $rcArgs = @($root.TrimEnd('\'), "$env:TEMP\mtech-migrate-null") + $filters + @('/S', '/L', '/XJ', '/R:0', '/W:0', '/NJH', '/NJS', '/NDL', '/NC', '/BYTES', '/TS', '/FP')
    if ($excludeDirs) { $rcArgs += '/XD'; $rcArgs += $excludeDirs }
    foreach ($line in @(& robocopy @rcArgs)) {
        if ($line -match '^\s*(\d+)\s+(\d{4}/\d\d/\d\d \d\d:\d\d:\d\d)\s+(.+?)\s*$') {
            [pscustomobject]@{ path = $matches[3]; bytes = [int64]$matches[1]; modified = $matches[2] }
        }
    }
}
# The signed-in user's profile folder.
function Console-Profile {
    $user = [string](Get-CimInstance Win32_ComputerSystem).UserName
    if (-not $user) { return '' }
    $sid = (New-Object Security.Principal.NTAccount($user)).Translate([Security.Principal.SecurityIdentifier]).Value
    if (-not $sid) { return '' }
    [string](Get-CimInstance Win32_UserProfile -Filter "SID='$sid'").LocalPath
}
# Firefox profile folders from profiles.ini, the default first.
function Firefox-Profiles([string]$firefoxDir) {
    $ini = Join-Path $firefoxDir 'profiles.ini'
    $sections = @()
    $cur = $null
    foreach ($line in @(Get-Content -LiteralPath $ini)) {
        if ($line -match '^\s*\[(.+)\]\s*$') { $cur = [ordered]@{ section = $matches[1] }; $sections += $cur; continue }
        if ($cur -and $line -match '^\s*([^=]+?)\s*=\s*(.*)$') { $cur[$matches[1]] = $matches[2] }
    }
    $installDefault = @($sections | Where-Object { $_.section -like 'Install*' -and $_.Default } | ForEach-Object { $_.Default })
    $dirs = foreach ($s in @($sections | Where-Object { $_.section -like 'Profile*' -and $_.Path })) {
        $path = if ($s.IsRelative -eq '1') { Join-Path $firefoxDir ($s.Path -replace '/', '\') } else { $s.Path }
        $rank = if ($installDefault -contains $s.Path) { 0 } elseif ($s.Default -eq '1') { 1 } else { 2 }
        [pscustomobject]@{ path = $path; rank = $rank }
    }
    @($dirs | Where-Object { Test-Path -LiteralPath $_.path } | Sort-Object rank | ForEach-Object { $_.path })
}
"##;
const SURVEY_PS: &str = r##"
$roots = @()
if ($OldRoot) { $roots = @($OldRoot.TrimEnd('\') + '\') }
else {
    foreach ($d in @(Get-PSDrive -PSProvider FileSystem)) {
        $r = [string]$d.Root
        if (-not $r -or $r -ieq $sys) { continue }
        if ((Test-Path -LiteralPath (Join-Path $r 'Windows\System32')) -and (Test-Path -LiteralPath (Join-Path $r 'Users'))) { $roots += $r }
    }
}

$oldDrives = foreach ($root in $roots) {
    $profiles = foreach ($p in @(Get-ChildItem -LiteralPath (Join-Path $root 'Users') -Directory -Force | Where-Object { $skipProfiles -notcontains $_.Name -and (Test-Path -LiteralPath (Join-Path $_.FullName 'AppData')) })) {
        $browsers = New-Object System.Collections.Generic.List[object]
        foreach ($b in $chromium.Keys) {
            $ud = Join-Path $p.FullName $chromium[$b][0]
            foreach ($pd in @(Get-ChildItem -LiteralPath $ud -Directory -Force | Where-Object { $_.Name -eq 'Default' -or $_.Name -like 'Profile *' })) {
                $browsers.Add([ordered]@{
                    browser = $b; profile = $pd.Name
                    bookmarks = Bookmark-Count (Join-Path $pd.FullName 'Bookmarks')
                    history_mb = MB (Size-Of (Join-Path $pd.FullName 'History'))
                    saved_logins_approx = Saved-Logins (Join-Path $pd.FullName 'Login Data')
                    extensions = @(Chromium-Extensions $pd.FullName | Select-Object -First 25)
                })
            }
        }
        foreach ($fp in @(Firefox-Profiles (Join-Path $p.FullName 'AppData\Roaming\Mozilla\Firefox'))) {
            $logins = Read-Json (Join-Path $fp 'logins.json')
            $browsers.Add([ordered]@{
                browser = 'firefox'; profile = (Split-Path $fp -Leaf)
                places_mb = MB (Size-Of (Join-Path $fp 'places.sqlite'))
                saved_logins = @($logins.logins).Count
                extensions = @(Firefox-Extensions $fp | Select-Object -First 25)
            })
        }
        $folders = @('Desktop', 'Documents', 'Downloads', 'Pictures', 'Music', 'Videos', 'Favorites') | Where-Object { Test-Path -LiteralPath (Join-Path $p.FullName $_) }
        $onedrive = @(Get-ChildItem -LiteralPath $p.FullName -Directory -Force -Filter 'OneDrive*' | ForEach-Object {
            [ordered]@{ name = $_.Name; folders = @(Get-ChildItem -LiteralPath $_.FullName -Directory -Force | ForEach-Object { $_.Name } | Select-Object -First 20) }
        })
        [ordered]@{
            name = $p.Name; path = $p.FullName; last_used = Day-Of (Join-Path $p.FullName 'NTUSER.DAT')
            folders = @($folders); onedrive = $onedrive; browsers = $browsers.ToArray()
            sticky_notes = [bool](Test-Path -LiteralPath (Join-Path $p.FullName 'AppData\Local\Packages\Microsoft.MicrosoftStickyNotes_8wekyb3d8bbwe\LocalState\plum.sqlite'))
            thunderbird = [bool](Test-Path -LiteralPath (Join-Path $p.FullName 'AppData\Roaming\Thunderbird\Profiles'))
        }
    }

    $installs = @(foreach ($pf in @('Program Files (x86)\Intuit', 'Program Files\Intuit')) {
        foreach ($d in @(Get-ChildItem -LiteralPath (Join-Path $root $pf) -Directory -Force | Where-Object { $_.Name -like 'QuickBooks*' })) {
            $exe = @(Get-ChildItem -LiteralPath $d.FullName -Filter 'QBW*.EXE' -File -Force | Select-Object -First 1)
            [ordered]@{ folder = $d.Name; version = if ($exe) { [string]$exe[0].VersionInfo.FileVersion } else { '' } }
        }
    })
    $registration = @()
    $reg = Join-Path $root 'ProgramData\Common Files\Intuit\QuickBooks\qbregistration.dat'
    if (Test-Path -LiteralPath $reg) {
        $xml = [xml](Get-Content -LiteralPath $reg -Raw)
        foreach ($v in @($xml.SelectNodes('//VERSION'))) {
            foreach ($f in @($v.SelectNodes('FLAVOR'))) {
                $lic = [string]$f.SelectSingleNode('LicenseNumber').InnerText
                $major = 0
                [void][int]::TryParse(([string]$v.GetAttribute('number')).Split('.')[0], [ref]$major)
                $registration += [ordered]@{
                    version = $v.GetAttribute('number'); year = if ($major -gt 0) { 1990 + $major } else { $null }; edition = $f.GetAttribute('name')
                    license_ends = $($digits = $lic -replace '[^0-9A-Za-z]', ''; if ($digits.Length -ge 4) { $digits.Substring($digits.Length - 4) } else { '' })
                }
            }
        }
    }
    $qbFiles = @(Find-Files $root $qbFilters $qbExcludes | Select-Object -First 40 | ForEach-Object { [ordered]@{ path = $_.path; mb = MB $_.bytes; modified = $_.modified } })
    $pst = @(Find-Files (Join-Path $root 'Users') @('*.pst') @('Temp') | Select-Object -First 20 | ForEach-Object { [ordered]@{ path = $_.path; mb = MB $_.bytes } })
    [ordered]@{
        root = $root; profiles = @($profiles)
        quickbooks = [ordered]@{
            installed = $installs; registration = $registration
            registration_file = [bool](Test-Path -LiteralPath $reg)
            files = $qbFiles
        }
        outlook_pst = $pst
    }
}

$targets = foreach ($p in @(Get-ChildItem -LiteralPath (Join-Path $sys 'Users') -Directory -Force | Where-Object { $skipProfiles -notcontains $_.Name -and (Test-Path -LiteralPath (Join-Path $_.FullName 'NTUSER.DAT')) })) {
    $logs = @(Get-ChildItem -Path (Join-Path $p.FullName 'Desktop\Robocopy-*.txt') -File -Force | ForEach-Object {
        $tail = @(Get-Content -LiteralPath $_.FullName -Tail 14 | Where-Object { $_ -match '^\s*(Dirs|Files|Bytes|Ended)\s*:' } | ForEach-Object { ($_ -replace '\s+', ' ').Trim() })
        [ordered]@{ name = $_.Name; summary = $tail }
    })
    $data = @(foreach ($b in $chromium.Keys) { if (Test-Path -LiteralPath (Join-Path $p.FullName $chromium[$b][0])) { $b } })
    if (Test-Path -LiteralPath (Join-Path $p.FullName 'AppData\Roaming\Mozilla\Firefox\Profiles')) { $data += 'firefox' }
    [ordered]@{
        name = $p.Name; path = $p.FullName
        users_backup = @(Get-ChildItem -LiteralPath (Join-Path $p.FullName 'Desktop\UsersBackup') -Directory -Force | ForEach-Object { $_.Name })
        robocopy_logs = $logs; browser_data = $data
    }
}
$newQb = @(Find-Files (Join-Path $sys 'Users') $qbFilters @('AppData', 'UsersBackup', 'Sample Company Files', 'QuickBooksAutoDataRecovery') | Select-Object -First 40 | ForEach-Object { [ordered]@{ path = $_.path; mb = MB $_.bytes; modified = $_.modified } })
$installed = @()
foreach ($b in ([ordered]@{ chrome = 'Google\Chrome\Application\chrome.exe'; edge = 'Microsoft\Edge\Application\msedge.exe'; brave = 'BraveSoftware\Brave-Browser\Application\brave.exe'; firefox = 'Mozilla Firefox\firefox.exe' }).GetEnumerator()) {
    foreach ($pf in @($env:ProgramFiles, ${env:ProgramFiles(x86)}, $env:LOCALAPPDATA)) {
        if ($pf -and (Test-Path -LiteralPath (Join-Path $pf $b.Value))) { $installed += $b.Key; break }
    }
}

[ordered]@{
    old_drives = @($oldDrives)
    this_pc = [ordered]@{ console_profile = Console-Profile; profiles = @($targets); quickbooks_files = $newQb; browsers_installed = $installed }
} | ConvertTo-Json -Compress -Depth 7
"##;
const BROWSERS_PS: &str = r##"
function Html-Node($node, $sb, [string]$indent) {
    $name = [System.Net.WebUtility]::HtmlEncode([string]$node.name)
    if ($node.type -eq 'url') {
        [void]$sb.AppendLine("$indent<DT><A HREF=`"$([System.Net.WebUtility]::HtmlEncode([string]$node.url))`">$name</A>")
    } elseif ($node.type -eq 'folder') {
        [void]$sb.AppendLine("$indent<DT><H3>$name</H3>")
        [void]$sb.AppendLine("$indent<DL><p>")
        foreach ($c in @($node.children)) { Html-Node $c $sb "$indent    " }
        [void]$sb.AppendLine("$indent</DL><p>")
    }
}
# Writes a Chromium Bookmarks file as a bookmarks HTML file any browser can import.
function Export-Html([string]$bookmarks, [string]$out) {
    $j = Read-Json $bookmarks
    $sb = New-Object System.Text.StringBuilder
    [void]$sb.AppendLine('<!DOCTYPE NETSCAPE-Bookmark-file-1>')
    [void]$sb.AppendLine('<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">')
    [void]$sb.AppendLine('<TITLE>Bookmarks</TITLE>')
    [void]$sb.AppendLine('<H1>Bookmarks</H1>')
    [void]$sb.AppendLine('<DL><p>')
    foreach ($r in @($j.roots.bookmark_bar, $j.roots.other, $j.roots.synced)) { if ($r) { Html-Node $r $sb '    ' } }
    [void]$sb.AppendLine('</DL><p>')
    [IO.File]::WriteAllText($out, $sb.ToString(), (New-Object Text.UTF8Encoding $false))
}
# Copies one SQLite or JSON file with its journal, removing a stale journal of the replaced file.
function Copy-Store([string]$from, [string]$to) {
    if (Test-Path -LiteralPath $to) { Copy-Item -LiteralPath $to -Destination "$to.mtech-bak" -Force }
    foreach ($suffix in @('-journal', '-wal', '-shm')) { Remove-Item -LiteralPath "$to$suffix" -Force }
    Copy-Item -LiteralPath $from -Destination $to -Force
    foreach ($suffix in @('-journal', '-wal')) {
        if (Test-Path -LiteralPath "$from$suffix") { Copy-Item -LiteralPath "$from$suffix" -Destination "$to$suffix" -Force }
    }
    Test-Path -LiteralPath $to
}

$src = $Source.TrimEnd('\')
$dst = if ($Target) { $Target.TrimEnd('\') } else { Console-Profile }
$results = New-Object System.Collections.Generic.List[object]
$problems = New-Object System.Collections.Generic.List[string]
if (-not (Test-Path -LiteralPath $src)) { $problems.Add("source profile not found: $src") }
if (-not $dst -or -not (Test-Path -LiteralPath $dst)) { $problems.Add("no target profile: pass target_profile (nobody is signed in)") }
$desktop = Join-Path $dst 'Desktop'
$chromiumFiles = @('Bookmarks', 'History', 'Favicons', 'Top Sites')

if ($problems.Count -eq 0) {
    foreach ($b in $chromium.Keys) {
        $srcUd = Join-Path $src $chromium[$b][0]
        $running = @(Get-Process -Name $chromium[$b][1]).Count -gt 0
        foreach ($pd in @(Get-ChildItem -LiteralPath $srcUd -Directory -Force | Where-Object { $_.Name -eq 'Default' -or $_.Name -like 'Profile *' })) {
            $row = [ordered]@{
                browser = $b; profile = $pd.Name; bookmarks = Bookmark-Count (Join-Path $pd.FullName 'Bookmarks')
                copied = @(); bookmarks_html = ''; extensions_left_out = @(Chromium-Extensions $pd.FullName | Select-Object -First 25); note = ''
            }
            $hasHistory = Test-Path -LiteralPath (Join-Path $pd.FullName 'History')
            if ($row.bookmarks -eq 0 -and -not $hasHistory) { $row.note = 'nothing to import'; $results.Add($row); continue }
            $dstDir = Join-Path (Join-Path $dst $chromium[$b][0]) $pd.Name
            $dstMarks = Bookmark-Count (Join-Path $dstDir 'Bookmarks')
            $direct = $pd.Name -eq 'Default' -and $dstMarks -eq 0 -and -not $running
            if ($row.bookmarks -gt 0 -and -not $direct) {
                $html = Join-Path $desktop "Bookmarks - $b - $($pd.Name).html"
                if (-not $DryRun) { Export-Html (Join-Path $pd.FullName 'Bookmarks') $html }
                $row.bookmarks_html = $html
            }
            if ($running) { $row.note = "$b is running: close it and run this again to bring over history; bookmarks were saved as HTML" }
            elseif ($pd.Name -ne 'Default') { $row.note = 'a second browser profile: import the HTML file into the matching profile' }
            elseif ($dstMarks -gt 0) { $row.note = "the new $b profile already has $dstMarks bookmarks: import the HTML file to merge" }
            if ($direct) {
                if (-not $DryRun) { $null = New-Item -ItemType Directory -Path $dstDir -Force }
                foreach ($f in $chromiumFiles) {
                    $from = Join-Path $pd.FullName $f
                    if (-not (Test-Path -LiteralPath $from)) { continue }
                    if ($DryRun -or (Copy-Store $from (Join-Path $dstDir $f))) { $row.copied += $f }
                }
            }
            $results.Add($row)
        }
    }

    $ffSrc = Firefox-Profiles (Join-Path $src 'AppData\Roaming\Mozilla\Firefox') | Select-Object -First 1
    if ($ffSrc) {
        $row = [ordered]@{ browser = 'firefox'; profile = (Split-Path $ffSrc -Leaf); copied = @(); extensions_left_out = @(Firefox-Extensions $ffSrc | Select-Object -First 25); note = '' }
        $ffDst = Join-Path $dst 'AppData\Roaming\Mozilla\Firefox'
        $existing = Firefox-Profiles $ffDst | Select-Object -First 1
        $fresh = $existing -and ((Get-Item -LiteralPath $existing -Force).CreationTime -gt (Get-Date).AddDays(-30))
        if (@(Get-Process -Name firefox).Count -gt 0) { $row.note = 'Firefox is running: close it and run this again' }
        elseif ($existing -and -not $fresh) { $row.note = "the new Firefox profile $(Split-Path $existing -Leaf) is older than 30 days; nothing was replaced" }
        else {
            $target = $existing
            if (-not $target) {
                $name = -join ((48..57) + (97..122) | Get-Random -Count 8 | ForEach-Object { [char]$_ })
                $target = Join-Path $ffDst "Profiles\$name.migrated"
                if (-not $DryRun) {
                    $null = New-Item -ItemType Directory -Path $target -Force
                    $ini = "[General]`r`nStartWithLastProfile=1`r`nVersion=2`r`n`r`n[Profile0]`r`nName=default-release`r`nIsRelative=1`r`nPath=Profiles/$name.migrated`r`nDefault=1`r`n"
                    [IO.File]::WriteAllText((Join-Path $ffDst 'profiles.ini'), $ini, (New-Object Text.UTF8Encoding $false))
                }
                $row.note = 'created a Firefox profile for the import'
            }
            foreach ($f in @('places.sqlite', 'favicons.sqlite', 'logins.json', 'key4.db')) {
                $from = Join-Path $ffSrc $f
                if (-not (Test-Path -LiteralPath $from)) { continue }
                if ($DryRun -or (Copy-Store $from (Join-Path $target $f))) { $row.copied += $f }
            }
        }
        $results.Add($row)
    }
}

[ordered]@{
    source = $src; target = $dst; dry_run = [bool]$DryRun; problems = $problems.ToArray(); imported = $results.ToArray()
    left_out = 'extensions, settings, search engines, cookies and Chrome/Edge saved passwords (tied to the old Windows account)'
} | ConvertTo-Json -Compress -Depth 6
"##;
const QUICKBOOKS_PS: &str = r##"
$dest = Join-Path $sys 'Users\Public\Documents\Intuit\QuickBooks\Company Files'
$limit = 3GB
$problems = New-Object System.Collections.Generic.List[string]
$found = @()
if ($Files.Count -gt 0) {
    foreach ($f in $Files) {
        $i = Get-Item -LiteralPath $f -Force
        if ($i) { $found += [pscustomobject]@{ path = $i.FullName; bytes = [int64]$i.Length; modified = $i.LastWriteTime.ToString('yyyy/MM/dd HH:mm:ss') } }
        else { $problems.Add("not found: $f") }
    }
} elseif ($OldRoot) {
    $found = @(Find-Files $OldRoot $qbFilters $qbExcludes)
} else {
    $problems.Add('pass old_root (the old drive, e.g. E:\) or files')
}

$plan = New-Object System.Collections.Generic.List[object]
foreach ($f in $found) {
    $plan.Add($f)
    if ($f.path -match '\.qbw$') {
        $dir = Split-Path $f.path
        $stem = [IO.Path]::GetFileNameWithoutExtension($f.path)
        $tlg = @(Get-ChildItem -LiteralPath $dir -File -Force | Where-Object { $_.Name -ieq "$stem.qbw.tlg" -or $_.Name -ieq "$stem.tlg" } | Select-Object -First 1)[0]
        if ($tlg) { $plan.Add([pscustomobject]@{ path = $tlg.FullName; bytes = [int64]$tlg.Length; modified = '' }) }
    }
}
$total = ($plan | Measure-Object -Property bytes -Sum).Sum
if ($total -gt $limit) {
    $problems.Add("$(MB $total) MB is more than this tool copies in one call; copy with robocopy over RemoteExec into $dest")
}

$copied = New-Object System.Collections.Generic.List[object]
$skipped = New-Object System.Collections.Generic.List[object]
if ($problems.Count -eq 0 -and $plan.Count -gt 0) {
    if (-not $DryRun) { $null = New-Item -ItemType Directory -Path $dest -Force }
    foreach ($f in $plan) {
        $name = Split-Path $f.path -Leaf
        $to = Join-Path $dest $name
        $there = Get-Item -LiteralPath $to -Force
        $from = Get-Item -LiteralPath $f.path -Force
        if ($there -and $there.Length -eq $from.Length -and $there.LastWriteTime -eq $from.LastWriteTime) {
            $skipped.Add([ordered]@{ name = $name; why = 'already here' }); continue
        }
        if ($there) {
            $to = Join-Path $dest ("{0} (old drive){1}" -f [IO.Path]::GetFileNameWithoutExtension($name), [IO.Path]::GetExtension($name))
        }
        if (-not $DryRun) { Copy-Item -LiteralPath $f.path -Destination $to -Force }
        if ($DryRun -or (Test-Path -LiteralPath $to)) { $copied.Add([ordered]@{ name = (Split-Path $to -Leaf); mb = MB $f.bytes; from = $f.path }) }
        else { $problems.Add("copy failed: $($f.path)") }
    }
}

$registration = @()
$installed = @()
if ($OldRoot) {
    $reg = Join-Path $OldRoot 'ProgramData\Common Files\Intuit\QuickBooks\qbregistration.dat'
    if (Test-Path -LiteralPath $reg) {
        $xml = [xml](Get-Content -LiteralPath $reg -Raw)
        foreach ($v in @($xml.SelectNodes('//VERSION'))) {
            foreach ($fl in @($v.SelectNodes('FLAVOR'))) {
                $lic = [string]$fl.SelectSingleNode('LicenseNumber').InnerText
                $major = 0
                [void][int]::TryParse(([string]$v.GetAttribute('number')).Split('.')[0], [ref]$major)
                $registration += [ordered]@{
                    version = $v.GetAttribute('number'); year = if ($major -gt 0) { 1990 + $major } else { $null }; edition = $fl.GetAttribute('name')
                    license_ends = $($digits = $lic -replace '[^0-9A-Za-z]', ''; if ($digits.Length -ge 4) { $digits.Substring($digits.Length - 4) } else { '' })
                }
            }
        }
    }
    foreach ($pf in @('Program Files (x86)\Intuit', 'Program Files\Intuit')) {
        foreach ($d in @(Get-ChildItem -LiteralPath (Join-Path $OldRoot $pf) -Directory -Force | Where-Object { $_.Name -like 'QuickBooks*' })) { $installed += $d.Name }
    }
}

[ordered]@{
    destination = $dest; dry_run = [bool]$DryRun; total_mb = MB $total
    copied = $copied.ToArray(); skipped = $skipped.ToArray(); problems = $problems.ToArray()
    old_install = @($installed); registration = @($registration)
    left_out = 'network data (.ND), DSN files and Auto Data Recovery copies; QuickBooks rebuilds them'
} | ConvertTo-Json -Compress -Depth 5
"##;

#[derive(Facet, Deserialize)]
struct SurveyArgs {
    /// Root of the old Windows drive, e.g. E:\ ; every attached Windows drive is surveyed when omitted.
    old_root: Option<String>,
}

#[derive(Facet, Deserialize)]
struct BrowserArgs {
    /// The old profile folder, e.g. E:\Users\sprou. Use the old drive: Data Transfer leaves browser profiles out of UsersBackup.
    source_profile: String,
    /// The new user's profile folder, e.g. C:\Users\Owner; the signed-in user's when omitted.
    target_profile: Option<String>,
    /// Report what would be imported without changing anything.
    dry_run: Option<bool>,
}

#[derive(Facet, Deserialize)]
struct QuickBooksArgs {
    /// Root of the old Windows drive, e.g. E:\ ; searched for company files and the QuickBooks registration.
    old_root: Option<String>,
    /// Exact .QBW, .QBB, .QBM or .QBX files to import instead of searching, e.g. from a USB drive.
    files: Option<Vec<String>>,
    /// Report what would be copied without copying.
    dry_run: Option<bool>,
}

/// A drive path as a single-quoted PowerShell literal; `None` for anything else.
fn ps_path(raw: &str) -> Option<String> {
    let v = raw.trim();
    let b = v.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
    let clean = !v.chars().any(|c| c.is_control() || matches!(c, '"' | '`' | '$' | ';' | '|' | '<' | '>' | '*' | '?'));
    (drive && clean && v.len() <= 260 && !v.contains("..")).then(|| format!("'{}'", v.replace('\'', "''")))
}

/// An optional drive path argument as a literal, `''` when absent.
fn optional_path(raw: Option<&str>, field: &str) -> Result<String, SdkError> {
    match raw.map(str::trim).filter(|r| !r.is_empty()) {
        Some(r) => {
            ps_path(r).ok_or_else(|| SdkError::invalid_args(format!("{field} must be a drive path such as E:\\")))
        }
        None => Ok("''".to_string()),
    }
}

fn ps_bool(v: Option<bool>) -> String {
    if v.unwrap_or(false) { "$true".into() } else { "$false".into() }
}

/// The shared helpers, then the variables, then the tool's script.
fn script(vars: &[(&str, String)], body: &str) -> String {
    let mut s = String::from(HELPERS_PS);
    for (name, value) in vars {
        s.push_str(&format!("\n${name} = {value}"));
    }
    s.push('\n');
    s.push_str(body);
    s
}

/// Parses PS JSON output into the tool envelope, stderr-safe.
fn envelope(tool: &str, out: String) -> serde_json::Value {
    let output = serde_json::from_str::<serde_json::Value>(out.trim()).unwrap_or(serde_json::Value::String(out));
    serde_json::json!({ "tool": tool, "output": output })
}

fn survey(a: SurveyArgs) -> Result<serde_json::Value, SdkError> {
    let root = optional_path(a.old_root.as_deref(), "old_root")?;
    host::log("[migrate] survey");
    Ok(envelope("survey", host::run_command(&script(&[("OldRoot", root)], SURVEY_PS))))
}

fn import_browsers(a: BrowserArgs) -> Result<serde_json::Value, SdkError> {
    let source = ps_path(&a.source_profile)
        .ok_or_else(|| SdkError::invalid_args("source_profile must be a profile folder such as E:\\Users\\name"))?;
    let target = optional_path(a.target_profile.as_deref(), "target_profile")?;
    host::log("[migrate] import_browsers");
    let vars = [("Source", source), ("Target", target), ("DryRun", ps_bool(a.dry_run))];
    Ok(envelope("import_browsers", host::run_command(&script(&vars, BROWSERS_PS))))
}

fn import_quickbooks(a: QuickBooksArgs) -> Result<serde_json::Value, SdkError> {
    let root = optional_path(a.old_root.as_deref(), "old_root")?;
    let mut files = Vec::new();
    for f in a.files.unwrap_or_default() {
        files.push(ps_path(&f).ok_or_else(|| SdkError::invalid_args(format!("not a drive path: {f}")))?);
    }
    host::log("[migrate] import_quickbooks");
    let vars = [("OldRoot", root), ("Files", format!("@({})", files.join(", "))), ("DryRun", ps_bool(a.dry_run))];
    Ok(envelope("import_quickbooks", host::run_command(&script(&vars, QUICKBOOKS_PS))))
}

mtech_plugin! {
    id: "com.mastertech.migrate",
    name: "Data Migration",
    version: "0.1.0",
    heap: 2 * 1024 * 1024,
    tools: {
        /// Read-only survey of what a data transfer has to move, in about a minute: each old Windows drive's profiles (folders, OneDrive, Sticky Notes, Thunderbird) and browsers (Chrome/Edge/Brave profiles with bookmark counts, history size, approximate saved logins and extension names; Firefox profiles), QuickBooks (installed year, registration with only the license's last 4 characters, company and backup files outside system folders), Outlook .pst files; and on this PC each profile's UsersBackup folders, Robocopy log summaries and any QuickBooks files.
        survey(SurveyArgs) => survey,
        /// Imports Chrome, Edge and Brave bookmarks and history and Firefox bookmarks, history and saved passwords from an old profile into the new user's profile. Copies only those data files, never extensions, settings, search engines or cookies, so hijackers and spam extensions stay behind; their names come back in extensions_left_out. A profile the browser already uses, a second browser profile or a running browser gets its bookmarks as an HTML file on the new Desktop instead. Chrome and Edge saved passwords cannot move (they are tied to the old Windows account): the customer signs in to browser sync. Replaced files keep a .mtech-bak copy.
        import_browsers(BrowserArgs) => import_browsers,
        /// Copies QuickBooks company files (.QBW with its .TLG), backups (.QBB), portable (.QBM) and accountant (.QBX) files from the old drive, or from the files given, into C:\Users\Public\Documents\Intuit\QuickBooks\Company Files. Skips files already there and renames on a name clash, never overwriting. Leaves out .ND, .DSN and Auto Data Recovery copies. Refuses more than 3 GB in one call. Also returns the old install's year and edition so the same version can be installed.
        import_quickbooks(QuickBooksArgs) => import_quickbooks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_drive_paths_become_literals() {
        assert_eq!(ps_path(r"E:\Users\O'Brien").as_deref(), Some(r"'E:\Users\O''Brien'"));
        assert_eq!(ps_path(" E:\\ ").as_deref(), Some(r"'E:\'"));
        for bad in [r"Users\x", r"E:\a$b", "E:\\a`b", r"E:\a;b", r"E:\..\x", "E:\\a\nb", r#"E:\a"b"#, r"\\server\share"]
        {
            assert!(ps_path(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn scripts_set_their_variables_between_the_helpers_and_the_body() {
        let s = script(&[("OldRoot", "'E:\\'".into()), ("DryRun", ps_bool(Some(true)))], "BODY");
        assert!(s.starts_with(HELPERS_PS));
        assert!(s.ends_with("\n$OldRoot = 'E:\\'\n$DryRun = $true\nBODY"), "{s}");
        assert_eq!(optional_path(None, "old_root").ok().as_deref(), Some("''"));
        assert!(optional_path(Some("nope"), "old_root").is_err());
    }
}
