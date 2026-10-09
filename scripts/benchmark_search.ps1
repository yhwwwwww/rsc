param(
    [ValidateRange(5, 1000)][int]$Runs = 20
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    New-Item -ItemType Directory -Force -Path '.test-lab' | Out-Null
    & hyperfine --warmup 5 --runs $Runs --shell none --export-json '.test-lab/hyperfine-search-installed.json' `
        'rsc search git' 'hok search git' 'hok search -B git' 'scoop.cmd search git'
    if ($LASTEXITCODE -ne 0) { throw 'Search benchmark failed.' }
} finally {
    Pop-Location
}
