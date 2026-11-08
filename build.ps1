$ErrorActionPreference = 'Stop'

$cargo = Get-Command cargo -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
if (!$cargo) {
    throw 'Cargo was not found on PATH. Install Rust and Cargo from https://rust-lang.org/tools/install/ and reopen PowerShell.'
}

$target = 'x86_64-pc-windows-msvc'
$targetDir = Join-Path $PSScriptRoot 'target'
$buildDir = Join-Path $PSScriptRoot 'build'

Push-Location -LiteralPath $PSScriptRoot
try {
    & $cargo.Source build --release --locked --target $target --target-dir $targetDir
    if ($LASTEXITCODE -ne 0) {
        throw 'Cargo build failed. See the error above. If the MSVC linker or Windows SDK is missing, install the Desktop development with C++ workload from https://visualstudio.microsoft.com/downloads/.'
    }

    New-Item -ItemType Directory -Path $buildDir -Force | Out-Null
    $exe = Join-Path $buildDir 'RustedGlass.exe'
    Copy-Item -LiteralPath (Join-Path $targetDir "$target/release/RustedGlass.exe") -Destination $exe
    Write-Output "Built $exe"
} finally {
    Pop-Location
}
