[CmdletBinding()]
param(
    [ValidateSet('Release', 'Debug')][string]$Configuration = 'Release',
    [string]$MinHookDir = '',
    [string]$Generator = 'Visual Studio 17 2022'
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if (-not $MinHookDir) { $MinHookDir = Join-Path $repo 'third_party/minhook' }
if (-not (Test-Path (Join-Path $MinHookDir 'include/MinHook.h'))) {
    throw "Extract MinHook v1.3.4 source to $MinHookDir; see docs/DOWNLOADS.md"
}
$MinHookDir = (Resolve-Path $MinHookDir).Path
Get-Command cmake -ErrorAction Stop | Out-Null
Get-Command cargo -ErrorAction Stop | Out-Null
$build = Join-Path $repo 'build/win32'
& cmake -S $repo -B $build -G $Generator -A Win32 "-DMINHOOK_DIR:PATH=$MinHookDir" -DRA3_BUILD_BRIDGE=ON
if ($LASTEXITCODE -ne 0) { throw 'CMake configure failed' }
& cmake --build $build --config $Configuration
if ($LASTEXITCODE -ne 0) { throw 'Native build failed' }
Push-Location $repo
try {
    $cargoArgs = @('build', '--locked', '--offline', '-p', 'ra3ctl')
    if ($Configuration -eq 'Release') { $cargoArgs += '--release' }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed' }
} finally { Pop-Location }
