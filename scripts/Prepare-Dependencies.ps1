[CmdletBinding()]
param(
    [string]$MinHookZip = '',
    [string]$LoaderZip = ''
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if (-not $MinHookZip) { $MinHookZip = Join-Path $repo 'downloads/minhook-1.3.4.zip' }
if (-not $LoaderZip) { $LoaderZip = Join-Path $repo 'downloads/Ultimate-ASI-Loader-NoPDB.zip' }

function Assert-FileHash([string]$Path, [string]$Expected) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing file: $Path" }
    if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ne $Expected) {
        throw "SHA-256 mismatch: $Path"
    }
}

# Pinned to the two archives received on 2026-10-17. No downloads or builds.
Assert-FileHash $MinHookZip '172708123daa0c98d20d3a980b16a50be14af243dc95dee6f79c24193ad010e4'
Assert-FileHash $LoaderZip '14b3a1ad018899571ac9aa01482977f3c6d49e6cba99f552d01c5acacd1315e1'
$MinHookZip = (Resolve-Path -LiteralPath $MinHookZip).Path
$LoaderZip = (Resolve-Path -LiteralPath $LoaderZip).Path

Add-Type -AssemblyName System.IO.Compression.FileSystem
$temp = Join-Path ([System.IO.Path]::GetTempPath()) ('ra3mcp-deps-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temp | Out-Null
try {
    [System.IO.Compression.ZipFile]::ExtractToDirectory($MinHookZip, (Join-Path $temp 'minhook'))
    [System.IO.Compression.ZipFile]::ExtractToDirectory($LoaderZip, (Join-Path $temp 'loader'))
    $source = Join-Path $temp 'minhook/minhook-1.3.4'
    foreach ($relative in @('include/MinHook.h', 'src/buffer.c', 'src/hook.c', 'src/trampoline.c', 'src/hde/hde32.c', 'LICENSE.txt')) {
        if (-not (Test-Path -LiteralPath (Join-Path $source $relative) -PathType Leaf)) {
            throw "Incomplete MinHook source: $relative"
        }
    }

    $loader = Join-Path $temp 'loader/dinput8.dll'
    Assert-FileHash $loader 'ec2f4824eca58dd40f425756a4a7cec77b8e381f21d11d7c846ec4b339b617ab'
    $bytes = [System.IO.File]::ReadAllBytes($loader)
    $pe = [System.BitConverter]::ToInt32($bytes, 0x3c)
    if ([System.BitConverter]::ToUInt32($bytes, $pe) -ne 0x00004550 -or
        [System.BitConverter]::ToUInt16($bytes, $pe + 4) -ne 0x014c) {
        throw 'Expected a Win32/x86 PE loader'
    }

    $destination = Join-Path $repo 'third_party/minhook'
    $loaderDestination = Join-Path $repo 'downloads/ual-win32/d3d9.dll'
    # Refuse to overwrite a different dependency or a locally modified source tree.
    if (Test-Path -LiteralPath $destination) {
        foreach ($file in Get-ChildItem -LiteralPath $source -File -Recurse -Force) {
            $relative = $file.FullName.Substring($source.Length + 1)
            Assert-FileHash (Join-Path $destination $relative) (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
        }
    }
    if (Test-Path -LiteralPath $loaderDestination) {
        Assert-FileHash $loaderDestination 'ec2f4824eca58dd40f425756a4a7cec77b8e381f21d11d7c846ec4b339b617ab'
    }
    if (-not (Test-Path -LiteralPath $destination)) {
        New-Item -ItemType Directory -Force (Split-Path -Parent $destination) | Out-Null
        Copy-Item -LiteralPath $source -Destination $destination -Recurse
    }
    if (-not (Test-Path -LiteralPath $loaderDestination)) {
        New-Item -ItemType Directory -Force (Split-Path -Parent $loaderDestination) | Out-Null
        Copy-Item -LiteralPath $loader -Destination $loaderDestination
    }
    Write-Output "MinHook source ready: $destination"
    Write-Output "Win32 loader ready: $loaderDestination"
    Write-Output 'Verified experiment kept root and Data copies. Game-side files: Data/winmm.dll, Data/global.ini, Data/scripts/RA3InputBridge.asi. See README for initialization diagnostics.'
    Write-Output 'No compilation or game installation performed. Build and test via local Toolhub.'
} finally {
    Remove-Item -LiteralPath $temp -Recurse -Force
}
