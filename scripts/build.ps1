param(
    [ValidateSet('release','debug')][string]$Profile = 'release',
    [string]$Target
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$variables = @('CARGO_HOME','RUSTUP_HOME','PATH','CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER')
$previous = @{}
foreach ($name in $variables) { $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
try {
    $portableCargo = Join-Path $projectRoot '.tools\cargo\bin\cargo.exe'
    if (Test-Path -LiteralPath $portableCargo) {
        $env:CARGO_HOME = Join-Path $projectRoot '.tools\cargo'
        $env:RUSTUP_HOME = Join-Path $projectRoot '.tools\rustup'
        $env:PATH = (Join-Path $env:CARGO_HOME 'bin') + ';' + $env:PATH
        $cargo = $portableCargo
        $compilerBin = Join-Path $projectRoot '.tools\w64devkit\bin'
        if (-not (Test-Path -LiteralPath (Join-Path $compilerBin 'gcc.exe'))) {
            throw 'Portable GNU compiler is missing. Run scripts\setup-dev.ps1.'
        }
        $env:PATH = $compilerBin + ';' + $env:PATH
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $compilerBin 'gcc.exe'
    } else {
        $cargo = (Get-Command cargo -ErrorAction Stop).Source
    }
    $arguments = @('build', '--locked', '--manifest-path', (Join-Path $projectRoot 'Cargo.toml'))
    if ($Profile -eq 'release') { $arguments += '--release' }
    if ($Target) { $arguments += @('--target', $Target) }
    Push-Location -LiteralPath $projectRoot
    try {
        & $cargo @arguments
        if ($LASTEXITCODE -ne 0) { throw 'rsc build failed.' }
    } finally { Pop-Location }
    $outputRoot = Join-Path $projectRoot 'target'
    if ($Target) { $outputRoot = Join-Path $outputRoot $Target }
    $artifact = Join-Path $outputRoot ($Profile + '\rsc.exe')
    $distributionRoot = Join-Path $projectRoot 'dist'
    New-Item -ItemType Directory -Force -Path $distributionRoot | Out-Null
    Copy-Item -LiteralPath $artifact -Destination (Join-Path $distributionRoot 'rsc.exe')
    Get-Item -LiteralPath (Join-Path $distributionRoot 'rsc.exe') | Select-Object FullName,Length
} finally {
    foreach ($name in $variables) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
}
