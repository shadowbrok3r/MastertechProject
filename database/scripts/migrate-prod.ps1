#Requires -Version 7
<#
.SYNOPSIS
    Apply a surrealkit rollout to production (surrealdb-dev.master-tech.app).

.DESCRIPTION
    Run from the workspace root (MastertechProject/).
    Reads DB_ROOT_USER, DB_ROOT_PASS, DB_URL_DEV, NS, DB from .env.
    If -RolloutId is omitted, auto-detects the newest pending manifest.

.PARAMETER RolloutId
    The bare rollout ID (no .toml extension).

.EXAMPLE
    .\database\scripts\migrate-prod.ps1
    .\database\scripts\migrate-prod.ps1 -RolloutId 20260516120000__tranche1_relax_fks
#>
[CmdletBinding()]
param(
    [string]$RolloutId
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RolloutsDir = 'database/rollouts'
$EnvFile     = '.env'

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
function Die([string]$Msg) { Write-Error "ERROR: $Msg"; exit 1 }
function Info([string]$Msg) { Write-Host ""; Write-Host ">>> $Msg" -ForegroundColor Cyan }

# Read a single KEY from the .env file
function Get-EnvValue([string]$Key) {
    $line = Get-Content $EnvFile -ErrorAction SilentlyContinue |
        Where-Object { $_ -match "^${Key}=(.*)$" } |
        Select-Object -First 1
    if ($line -match "^${Key}=(.*)$") { return $Matches[1].Trim() }
    return $null
}

# ---------------------------------------------------------------------------
# Pre-flight
# ---------------------------------------------------------------------------
if (-not (Test-Path 'surrealkit.toml')) { Die 'Run this script from the workspace root (MastertechProject/)' }
if (-not (Test-Path $EnvFile))          { Die ".env not found at workspace root" }
if (-not (Get-Command surrealkit -ErrorAction SilentlyContinue)) {
    Die 'surrealkit not found. Install with: cargo binstall surrealkit'
}

# ---------------------------------------------------------------------------
# Load connection values from .env
# ---------------------------------------------------------------------------
$DbRootUser = Get-EnvValue 'DB_ROOT_USER'
$DbRootPass = Get-EnvValue 'DB_ROOT_PASS'
$DbUrlDev   = Get-EnvValue 'DB_URL_DEV'
$Ns         = Get-EnvValue 'NS'
$Db         = Get-EnvValue 'DB'

if (-not $DbRootUser) { Die "DB_ROOT_USER not set in $EnvFile" }
if (-not $DbRootPass) { Die "DB_ROOT_PASS not set in $EnvFile" }
if (-not $DbUrlDev)   { Die "DB_URL_DEV not set in $EnvFile" }
if (-not $Ns)         { Die "NS not set in $EnvFile" }
if (-not $Db)         { Die "DB not set in $EnvFile" }

$ConnArgs = @(
    '--host', "wss://$DbUrlDev",
    '--ns',   $Ns,
    '--db',   $Db,
    '--user', $DbRootUser,
    '--pass', $DbRootPass
)

# ---------------------------------------------------------------------------
# Resolve rollout ID
# ---------------------------------------------------------------------------
if (-not $RolloutId) {
    $PendingManifest = Get-ChildItem "$RolloutsDir/*.toml" -ErrorAction SilentlyContinue |
        Where-Object { (Get-Content $_.FullName -Raw) -match '(?m)^state = "(planned|ready_to_complete)"' } |
        Sort-Object Name |
        Select-Object -First 1

    if (-not $PendingManifest) {
        Write-Host "No pending rollout manifests found in $RolloutsDir/. Nothing to do."
        exit 0
    }
    $RolloutId = [System.IO.Path]::GetFileNameWithoutExtension($PendingManifest.Name)
    Write-Host "Auto-detected rollout: $RolloutId"
}

$ManifestPath = "$RolloutsDir/$RolloutId.toml"
if (-not (Test-Path $ManifestPath)) { Die "Manifest not found: $ManifestPath" }

# ---------------------------------------------------------------------------
# Read current state from manifest
# ---------------------------------------------------------------------------
$ManifestContent = Get-Content $ManifestPath -Raw
if ($ManifestContent -match '(?m)^state = "([^"]+)"') {
    $CurrentState = $Matches[1]
} else {
    Die "Could not read state from $ManifestPath"
}

Write-Host ""
Write-Host "Rollout : $RolloutId"
Write-Host "State   : $CurrentState"
Write-Host "Target  : wss://$DbUrlDev / $Ns / $Db"
Write-Host ""

if ($CurrentState -eq 'completed') {
    Write-Host "Already completed — nothing to do."
    exit 0
}

# ---------------------------------------------------------------------------
# Production confirmation prompt
# ---------------------------------------------------------------------------
Write-Host '  *** PRODUCTION DATABASE ***' -ForegroundColor Yellow
Write-Host ''
$Confirm = Read-Host "Type 'yes' to continue"
if ($Confirm -ne 'yes') {
    Write-Host "Aborted."
    exit 1
}

# ---------------------------------------------------------------------------
# Run rollout
# ---------------------------------------------------------------------------
switch ($CurrentState) {
    'planned' {
        Info 'Lint'
        & surrealkit @ConnArgs rollout lint $ManifestPath
        if ($LASTEXITCODE -ne 0) { Die 'Lint failed. Fix the manifest before applying.' }

        Info 'Start phase'
        & surrealkit @ConnArgs rollout start $RolloutId
        if ($LASTEXITCODE -ne 0) { Die 'rollout start failed.' }

        Info 'Complete phase'
        & surrealkit @ConnArgs rollout complete $RolloutId
        if ($LASTEXITCODE -ne 0) { Die 'rollout complete failed.' }
    }
    { $_ -in 'ready_to_complete', 'running_start' } {
        Info 'Complete phase (resuming)'
        & surrealkit @ConnArgs rollout complete $RolloutId
        if ($LASTEXITCODE -ne 0) { Die 'rollout complete failed.' }
    }
    default {
        Die "Unexpected rollout state: '$CurrentState'. Inspect $ManifestPath manually."
    }
}

Write-Host ""
Write-Host "=== Migration complete ===" -ForegroundColor Green
Write-Host "Verify with: INFO FOR TABLE <table>;"
