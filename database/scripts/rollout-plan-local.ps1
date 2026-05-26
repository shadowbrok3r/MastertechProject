#Requires -Version 7
<#
.SYNOPSIS
    Generate a surrealkit rollout plan against localhost.

.DESCRIPTION
    Run from the workspace root (MastertechProject/).
    Reads DB_ROOT_USER, DB_ROOT_PASS, DB_URL_LOCAL, NS, DB from .env.
    Diffs the current local DB schema against the .surql files and writes
    a new manifest to database/rollouts/.

.PARAMETER Name
    Name for the rollout (e.g. slice10_embeddings). Required.

.EXAMPLE
    .\database\scripts\rollout-plan-local.ps1 -Name slice10_embeddings
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Name
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$EnvFile = '.env'

function Die([string]$Msg) { Write-Error "ERROR: $Msg"; exit 1 }
function Info([string]$Msg) { Write-Host ""; Write-Host ">>> $Msg" -ForegroundColor Cyan }

function Get-EnvValue([string]$Key) {
    $line = Get-Content $EnvFile -ErrorAction SilentlyContinue |
        Where-Object { $_ -match "^${Key}=(.*)$" } |
        Select-Object -First 1
    if ($line -match "^${Key}=(.*)$") { return $Matches[1].Trim() }
    return $null
}

if (-not (Test-Path 'surrealkit.toml')) { Die 'Run this script from the workspace root (MastertechProject/)' }
if (-not (Test-Path $EnvFile))          { Die ".env not found at workspace root" }
if (-not (Get-Command surrealkit -ErrorAction SilentlyContinue)) {
    Die 'surrealkit not found. Install with: cargo binstall surrealkit'
}

$DbRootUser = Get-EnvValue 'DB_ROOT_USER'
$DbRootPass = Get-EnvValue 'DB_ROOT_PASS'
$DbUrlLocal = Get-EnvValue 'DB_URL_LOCAL'
$Ns         = Get-EnvValue 'NS'
$Db         = Get-EnvValue 'DB'

if (-not $DbRootUser) { Die "DB_ROOT_USER not set in $EnvFile" }
if (-not $DbRootPass) { Die "DB_ROOT_PASS not set in $EnvFile" }
if (-not $DbUrlLocal) { Die "DB_URL_LOCAL not set in $EnvFile" }
if (-not $Ns)         { Die "NS not set in $EnvFile" }
if (-not $Db)         { Die "DB not set in $EnvFile" }

Info "Planning rollout '$Name' against ws://$DbUrlLocal"

& surrealkit `
    --host "ws://$DbUrlLocal" `
    --ns   $Ns `
    --db   $Db `
    --user $DbRootUser `
    --pass $DbRootPass `
    rollout plan --name $Name

if ($LASTEXITCODE -ne 0) { Die 'rollout plan failed.' }

Write-Host ""
Write-Host "Manifest written to database/rollouts/." -ForegroundColor Green
Write-Host "Review the generated DDL, replace auto-generated statements with"
Write-Host "custom DDL if needed (VALUE fn::embed_text, HNSW params, backfills),"
Write-Host "then run: .\database\scripts\migrate-local.ps1"
