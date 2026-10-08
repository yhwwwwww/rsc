$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$toolsRoot = Join-Path $projectRoot '.tools'
New-Item -ItemType Directory -Force -Path $toolsRoot | Out-Null
$previousCargo = [Environment]::GetEnvironmentVariable('CARGO_HOME', 'Process')
$previousRustup = [Environment]::GetEnvironmentVariable('RUSTUP_HOME', 'Process')
try {
    $env:CARGO_HOME = Join-Path $toolsRoot 'cargo'
    $env:RUSTUP_HOME = Join-Path $toolsRoot 'rustup'
    $cargoPath = Join-Path $env:CARGO_HOME 'bin\cargo.exe'
    if (-not (Test-Path -LiteralPath $cargoPath)) {
        $installer = Join-Path $toolsRoot 'rustup-init.exe'
        $checksumFile = $installer + '.sha256'
        $url = 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-gnu/rustup-init.exe'
        Invoke-WebRequest -Uri $url -OutFile $installer
        Invoke-WebRequest -Uri ($url + '.sha256') -OutFile $checksumFile
        $expected = [System.IO.File]::ReadAllText($checksumFile).Trim().Split(' ')[0]
        if ((Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash -ne $expected) { throw 'Rust installer checksum mismatch.' }
        & $installer -y --no-modify-path --profile minimal --default-host x86_64-pc-windows-gnu --default-toolchain stable
        if ($LASTEXITCODE -ne 0) { throw 'Rust setup failed.' }
    }
    $gccPath = Join-Path $toolsRoot 'w64devkit\bin\gcc.exe'
    if (-not (Test-Path -LiteralPath $gccPath)) {
        $sevenZip = (Get-Command 7z.exe -ErrorAction Stop).Source
        $archive = Join-Path $toolsRoot 'w64devkit-x64-2.10.0.7z.exe'
        Invoke-WebRequest -Uri 'https://github.com/skeeto/w64devkit/releases/download/v2.10.0/w64devkit-x64-2.10.0.7z.exe' -OutFile $archive
        $expected = '18D0A4C71A166F8401AB6305781BEC5882B40B5E06BA9807C61CB5F3B3C6325E'
        if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expected) { throw 'Compiler archive checksum mismatch.' }
        & $sevenZip x $archive ('-o' + $toolsRoot) -y -bso0 -bsp0
        if ($LASTEXITCODE -ne 0) { throw 'Compiler extraction failed.' }
    }
    & (Join-Path $env:CARGO_HOME 'bin\rustup.exe') component add rustfmt
    if ($LASTEXITCODE -ne 0) { throw 'Formatter setup failed.' }
    Write-Output 'Development tools are ready in .tools. Build with scripts\build.ps1.'
} finally {
    [Environment]::SetEnvironmentVariable('CARGO_HOME', $previousCargo, 'Process')
    [Environment]::SetEnvironmentVariable('RUSTUP_HOME', $previousRustup, 'Process')
}
