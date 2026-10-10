param(
    [ValidateSet('all','network','lifecycle','real','native')][string]$Phase='all',
    [switch]$SkipBuild
)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$previous=@{}
foreach($name in @('CARGO_HOME','RUSTUP_HOME','PATH','CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER')) {
    $previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')
}
Push-Location -LiteralPath $projectRoot
try {
    $portableCargo=Join-Path $projectRoot '.tools\cargo\bin\cargo.exe'
    if(Test-Path -LiteralPath $portableCargo) {
        $env:CARGO_HOME=Join-Path $projectRoot '.tools\cargo'
        $env:RUSTUP_HOME=Join-Path $projectRoot '.tools\rustup'
        $env:PATH=(Join-Path $projectRoot '.tools\w64devkit\bin')+';'+(Join-Path $env:CARGO_HOME 'bin')+';'+$env:PATH
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=Join-Path $projectRoot '.tools\w64devkit\bin\gcc.exe'
    }
    $python=(Get-Command python -ErrorAction Stop).Source
    $cargo=(Get-Command cargo -ErrorAction Stop).Source
    $rustc=(Get-Command rustc -ErrorAction Stop).Source
    & $python (Join-Path $PSScriptRoot 'version.py') --sync | Out-Null
    if($LASTEXITCODE -ne 0) { throw 'Cannot synchronize the Git version.' }
    & $cargo test --locked
    if($LASTEXITCODE -ne 0) { throw 'Rust regression tests failed.' }
    if(!$SkipBuild) { & (Join-Path $PSScriptRoot 'build.ps1') -Profile release }
    if(!(Test-Path -LiteralPath 'dist\rsc.exe')) { throw 'Build dist\rsc.exe first.' }
    New-Item -ItemType Directory -Force -Path '.test-lab' | Out-Null
    $fixtureArgs=@('tests\fixtures\echo.rs','-o','.test-lab\fixture.exe','-C','target-feature=+crt-static')
    if((& $rustc -vV) -match 'host: .*windows-gnu') {
        $fixtureArgs+=@('-C','link-self-contained=yes')
        if($env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER) {
            $fixtureArgs+=@('-C',('linker='+$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER))
        }
    }
    & $rustc @fixtureArgs
    if($LASTEXITCODE -ne 0) { throw 'Test fixture build failed.' }
    $guiArgs=$fixtureArgs.Clone()
    $guiArgs[0]='tests\fixtures\gui.rs'
    $guiArgs[2]='.test-lab\gui.exe'
    & $rustc @guiArgs
    if($LASTEXITCODE -ne 0) { throw 'GUI fixture build failed.' }
    & $python 'tests\windows_smoke.py' --phase $Phase
    if($LASTEXITCODE -ne 0) { throw 'Windows integration tests failed. See .test-lab\latest.json.' }
} finally {
    Pop-Location
    foreach($name in $previous.Keys) { [Environment]::SetEnvironmentVariable($name,$previous[$name],'Process') }
}
