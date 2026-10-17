[CmdletBinding()]
param(
    [ValidateSet('Release', 'Debug')][string]$Configuration = 'Release',
    [string]$MinHookDir = ''
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if (-not $MinHookDir) { $MinHookDir = Join-Path $repo 'third_party/minhook' }
$profile = if ($Configuration -eq 'Release') { 'release' } else { 'debug' }
$plugin = Join-Path $repo "build/win32/$Configuration/RA3InputBridge.asi"
$controller = Join-Path $repo "target/$profile/ra3ctl.exe"
$license = Join-Path $MinHookDir 'LICENSE.txt'
foreach ($file in @($plugin, $controller, $license)) {
    if (-not (Test-Path $file)) { throw "Missing $file; build locally first" }
}
$stage = Join-Path $repo "dist/$Configuration"
New-Item -ItemType Directory -Force (Join-Path $stage 'scripts') | Out-Null
New-Item -ItemType Directory -Force (Join-Path $stage 'licenses') | Out-Null
Copy-Item $plugin (Join-Path $stage 'scripts/RA3InputBridge.asi') -Force
Copy-Item $controller (Join-Path $stage 'ra3ctl.exe') -Force
Copy-Item $license (Join-Path $stage 'licenses/MinHook-LICENSE.txt') -Force
Copy-Item (Join-Path $repo 'LICENSE') (Join-Path $stage 'licenses/RA3MCP-LICENSE.txt') -Force
Copy-Item (Join-Path $repo 'config/global.ini.example') (Join-Path $stage 'global.ini.example') -Force
Copy-Item (Join-Path $repo 'README.md') (Join-Path $stage 'README.md') -Force
Write-Output "Staged: $stage"
Write-Output 'Deploy to the game root containing RA3.exe: d3d9.dll, global.ini, scripts/RA3InputBridge.asi.'
Write-Output 'Copy the verified loader from downloads/ual-win32/d3d9.dll; merge global.ini.example as global.ini.'
Write-Output 'Runtime log: <game root>/scripts/RA3InputBridge.log. This staging script does not install game files.'
