<#
.SYNOPSIS
    Builds Kuro and its setup on this computer, as the release does.

.DESCRIPTION
    Writes to dist\ (ignored by git):
        kuro-<version>-windows-x64-setup.exe
        kuro-<version>-windows-x64.zip       (with -Zip)
        SHA256SUMS
    Needs Rust and cargo-about. The version comes from Cargo.toml. Client ids are taken from
    KURO_ANILIST_CLIENT_ID, KURO_MAL_CLIENT_ID, KURO_GOOGLE_CLIENT_ID and
    KURO_GOOGLE_CLIENT_SECRET when set, as in the release.

.EXAMPLE
    packaging\windows\build.ps1
.EXAMPLE
    packaging\windows\build.ps1 -Zip -Out C:\temp\kuro
#>
param(
    [string]$Out = 'dist',
    # Also the program without a setup, like the release.
    [switch]$Zip
)
$ErrorActionPreference = 'Stop'
Set-Location (Resolve-Path "$PSScriptRoot\..\..")

# Cargo reports progress on stderr, which Windows PowerShell would take for an error.
function Invoke-Cargo([string]$what) {
    $ErrorActionPreference = 'Continue'
    & cargo @args
    if ($LASTEXITCODE) { throw "$what failed ($LASTEXITCODE)" }
}

if (-not (Get-Command cargo-about -ErrorAction SilentlyContinue)) {
    throw 'cargo-about is missing: cargo install cargo-about --locked --version 0.9.3 --features cli'
}
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$name = "kuro-$version-windows-x64"
Write-Host "Kuro $version" -ForegroundColor Cyan

Invoke-Cargo 'Building Kuro' build --release --locked -p kuro
$target = (cargo metadata --format-version 1 --no-deps | ConvertFrom-Json).target_directory
$release = Join-Path $target 'release'

# What goes inside the setup.
$payload = Join-Path $target 'payload'
Remove-Item $payload -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $payload | Out-Null
Copy-Item "$release\kuro.exe" $payload
Invoke-Cargo 'Listing the licences' about generate --locked --fail -o "$payload\THIRD-PARTY-LICENSES.html" about.hbs

$env:KURO_PAYLOAD = $payload
try {
    Invoke-Cargo 'Building the setup' build --release --locked -p kuro-setup
} finally {
    Remove-Item Env:KURO_PAYLOAD
}

New-Item -ItemType Directory $Out -Force | Out-Null
Get-ChildItem $Out -Filter "kuro-*" | Remove-Item -Recurse -Force
Copy-Item "$release\kuro-setup.exe" "$Out\$name-setup.exe"
if ($Zip) {
    $folder = Join-Path $target $name
    Remove-Item $folder -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory $folder | Out-Null
    Copy-Item "$payload\*", README.md, crates\app\assets\fonts\PHOSPHOR-LICENSE.txt $folder
    # Windows PowerShell's zip tools write paths with backslashes; other systems expect "/".
    Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
    $archive = Join-Path (Resolve-Path $Out) "$name.zip"
    $z = [IO.Compression.ZipFile]::Open($archive, 'Create')
    try {
        foreach ($f in Get-ChildItem $folder -File) {
            [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($z, $f.FullName, "$name/$($f.Name)", 'Optimal') | Out-Null
        }
    } finally {
        $z.Dispose()
    }
}

# Same format as sha256sum, line endings included, so `sha256sum -c SHA256SUMS` works.
$sums = Get-ChildItem $Out -Filter "kuro-*" | ForEach-Object {
    '{0} *{1}' -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
}
[IO.File]::WriteAllText((Join-Path (Resolve-Path $Out) 'SHA256SUMS'), (($sums -join "`n") + "`n"))

Get-ChildItem $Out | ForEach-Object { '{0,-45} {1,8:N1} MB' -f $_.Name, ($_.Length / 1MB) }
