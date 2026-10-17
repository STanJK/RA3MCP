[CmdletBinding()]
param([ValidateSet('Release', 'Debug')][string]$Configuration = 'Release')
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$build = Join-Path $repo 'build/win32'
if (-not (Test-Path (Join-Path $build 'CMakeCache.txt'))) {
    throw 'Run scripts/Build.ps1 locally first'
}
& ctest --test-dir $build -C $Configuration --output-on-failure
if ($LASTEXITCODE -ne 0) { throw 'Native protocol/queue tests failed' }
Push-Location $repo
try {
    & cargo test --locked --offline -p ra3ctl
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed' }
} finally { Pop-Location }
