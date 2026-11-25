# Installs, updates and uninstalls Kuro for the current user and checks what is left.
# It removes a Kuro already installed here, so it only runs on a disposable machine.
param(
    # Holds the installers of these three versions.
    [Parameter(Mandatory)][string]$Folder,
    [Parameter(Mandatory)][string]$Older,
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$Newer
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Only for GitHub Actions: it uninstalls Kuro' }

$app = "$env:LOCALAPPDATA\Programs\Kuro"
$exe = "$app\kuro.exe"
$data = "$env:LOCALAPPDATA\Kuro\data\kuro.db"
$run = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$link = 'HKCU:\Software\Classes\kuro'
$shortcut = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Kuro.lnk"
$listed = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{430942B2-A6DE-46B7-96A7-DDF405974F13}_is1'

function Assert([bool]$ok, [string]$what) {
    if (-not $ok) { throw "FAILED: $what" }
    Write-Host "ok: $what"
}
function Invoke-Quiet([string]$file, [int]$expect = 0) {
    $name = Split-Path $file -Leaf
    $p = Start-Process $file -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -PassThru
    Assert ($p.WaitForExit(120000)) "$name ends"
    Assert ($p.ExitCode -eq $expect) "$name ends with $expect ($($p.ExitCode))"
}
function Install-Kuro([string]$v, [int]$expect = 0) {
    Invoke-Quiet "$Folder\kuro-$v-windows-x64-setup.exe" $expect
}
function Assert-Version([string]$v) {
    $is = (Get-ItemProperty $listed).DisplayVersion
    Assert ($is -eq $v) "the installed version is $v ($is)"
}
function Start-Kuro {
    $p = Start-Process $exe -ArgumentList '--hidden' -PassThru
    Start-Sleep 10
    Assert (-not $p.HasExited) 'Kuro runs'
    $p
}
function Uninstall-Kuro {
    Invoke-Quiet "$app\unins000.exe"
    # The uninstaller finishes from a copy of itself.
    for ($i = 0; $i -lt 60 -and (Test-Path $app); $i++) { Start-Sleep 1 }
    Assert (-not (Test-Path $app)) 'uninstalling removes the program'
}

Install-Kuro $Older
Assert (Test-Path $exe) 'the program is installed for this user'
Assert (Test-Path "$app\THIRD-PARTY-LICENSES.html") 'the licences come with it'
Assert (Test-Path $shortcut) 'it is in the Start menu'
Assert (Test-Path $listed) 'Windows lists it'
Assert-Version $Older

$kuro = Start-Kuro
Assert (Test-Path $data) 'Kuro made its database'
# Checked apart: the installer could also close it by force.
$quit = Start-Process $exe -ArgumentList '--quit' -PassThru
Assert ($quit.WaitForExit(20000) -and $quit.ExitCode -eq 0) 'Kuro is asked to quit'
Assert ($kuro.WaitForExit(5000)) 'and it does'

$kuro = Start-Kuro
Install-Kuro $Version
Assert ($kuro.WaitForExit(5000)) 'an update closes Kuro'
Assert (Test-Path $exe) 'the update leaves the program in place'
Assert-Version $Version

Install-Kuro $Version
Assert-Version $Version

Install-Kuro $Older 1
Assert-Version $Version
Install-Kuro $Newer
Assert-Version $Newer
Install-Kuro $Version 1
Assert-Version $Newer

# A folder deleted by hand is not an installed version.
Remove-Item $exe
Install-Kuro $Version
Assert (Test-Path $exe) 'a missing program is installed again'
Assert-Version $Version

$kuro = Start-Kuro
New-Item $run -ErrorAction SilentlyContinue | Out-Null
Set-ItemProperty $run Kuro "`"$exe`" --hidden"
New-Item "$link\shell\open\command" -Force -Value "`"$exe`" `"%1`"" | Out-Null
Uninstall-Kuro
Assert ($kuro.WaitForExit(5000)) 'uninstalling closes Kuro'
Assert (-not (Test-Path $shortcut)) 'the shortcut is gone'
Assert (-not (Test-Path $listed)) 'Windows no longer lists it'
Assert ($null -eq (Get-Item $run).GetValue('Kuro')) 'it no longer starts with Windows'
Assert (-not (Test-Path $link)) 'kuro:// links are released'
Assert (Test-Path $data) 'the list is kept'

# Entries of another copy are not the installer's to remove.
Install-Kuro $Version
Set-ItemProperty $run Kuro '"C:\Other\kuro.exe" --hidden'
Uninstall-Kuro
Assert ((Get-Item $run).GetValue('Kuro') -like '*Other*') 'the entry of another copy is left alone'
