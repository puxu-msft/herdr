param(
    [Parameter(Mandatory = $true)]
    [string]$HerdrExe,

    [Parameter(Mandatory = $true)]
    [string]$PackagePath,

    [Parameter(Mandatory = $true)]
    [string]$StageDir,

    [Parameter(Mandatory = $true)]
    [string]$OutputPath,

    [ValidateSet("x86_64", "arm64")]
    [string]$Architecture = "x86_64"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Invoke-NativeChecked {
    param(
        [string]$Command,
        [string[]]$Arguments
    )

    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command failed with exit code $LASTEXITCODE"
    }
}

$packager = Join-Path $PSScriptRoot "package_windows_conpty.py"
Invoke-NativeChecked python @(
    $packager,
    "stage",
    "--architecture", $Architecture,
    "--package", $PackagePath,
    "--herdr-exe", $HerdrExe,
    "--output-dir", $StageDir
)
Invoke-NativeChecked dotnet @("nuget", "verify", "--all", $PackagePath)

$conptyManifestPath = Join-Path $PSScriptRoot "..\packaging\windows\conpty.json"
$conptyManifest = Get-Content -Raw -LiteralPath $conptyManifestPath | ConvertFrom-Json
$signedFiles = @($conptyManifest.bundles.$Architecture.files | ForEach-Object { $_.destination -replace "/", "\" })
if ($signedFiles.Count -eq 0) {
    throw "No ConPTY bundle files are declared for $Architecture in $conptyManifestPath"
}
foreach ($relative in $signedFiles) {
    $signature = Get-AuthenticodeSignature (Join-Path $StageDir $relative)
    $subject = if ($null -eq $signature.SignerCertificate) { "" } else { $signature.SignerCertificate.Subject }
    if ($signature.Status -ne "Valid" -or $subject -notlike "*Microsoft Corporation*") {
        throw "Invalid Microsoft signature for $relative`: $($signature.Status) $subject"
    }
}

Invoke-NativeChecked python @(
    $packager,
    "archive",
    "--architecture", $Architecture,
    "--stage-dir", $StageDir,
    "--output", $OutputPath
)
