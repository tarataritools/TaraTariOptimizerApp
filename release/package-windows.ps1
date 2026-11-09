param(
    [Parameter(Mandatory = $true)]
    [string]$DotnetRoot,
    [string]$Version = "0.0.1"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
$targets = @(
    @{ Name = "x64"; Rust = "x86_64-pc-windows-msvc"; Runtime = "win-x64" },
    @{ Name = "x86"; Rust = "i686-pc-windows-msvc"; Runtime = "win-x86" }
)
$targetDir = Join-Path $root "target\release-package"
$outputDir = Join-Path $root "dist\release-$Version"
$wxs = Join-Path $root "installer\TaraTariOptimizer.wxs"
$wixDir = Join-Path $root "target\wix-cli-v5"

if (-not (Test-Path $cargo)) {
    throw "Rust Cargo was not found at $cargo."
}
if (-not $DotnetRoot -or -not (Test-Path (Join-Path $DotnetRoot "dotnet.exe"))) {
    throw "Set DOTNET_ROOT to a .NET 8 SDK installation directory."
}

$env:PATH = "$DotnetRoot;$env:PATH"
$env:DOTNET_ROOT = $DotnetRoot
$env:CARGO_TARGET_DIR = $targetDir
New-Item -ItemType Directory -Force $outputDir | Out-Null

if (-not (Test-Path (Join-Path $wixDir "wix.exe"))) {
    & (Join-Path $DotnetRoot "dotnet.exe") tool install wix --version 5.0.2 --tool-path $wixDir
    if ($LASTEXITCODE -ne 0) {
        throw "Could not install the WiX Toolset CLI."
    }
}
$wix = Join-Path $wixDir "wix.exe"

foreach ($target in $targets) {
    & rustup target add $target.Rust
    if ($LASTEXITCODE -ne 0) {
        throw "Could not install Rust target $($target.Rust)."
    }

    & $cargo build --locked --release --target $target.Rust
    if ($LASTEXITCODE -ne 0) {
        throw "Release build failed for $($target.Name)."
    }

    $buildDir = Join-Path $targetDir "$($target.Rust)\release"
    $app = Join-Path $buildDir "tara-tari-optimizer.exe"
    $bridgeDir = Join-Path $buildDir "hardware-bridge"
    $bridge = Join-Path $bridgeDir "TaraTariHardwareBridge.exe"
    if (-not (Test-Path $app) -or -not (Test-Path $bridge)) {
        throw "The app or LibreHardwareMonitor bridge is missing for $($target.Name)."
    }

    $stageDir = Join-Path $outputDir "stage-$($target.Name)"
    $portableDir = Join-Path $outputDir "portable-$($target.Name)"
    New-Item -ItemType Directory -Force (Join-Path $stageDir "hardware-bridge") | Out-Null
    New-Item -ItemType Directory -Force (Join-Path $portableDir "hardware-bridge") | Out-Null
    Copy-Item -LiteralPath $app -Destination (Join-Path $stageDir "TaraTariOptimizerApp.exe") -Force
    Copy-Item -LiteralPath $app -Destination (Join-Path $portableDir "TaraTariOptimizerApp.exe") -Force
    Copy-Item -LiteralPath $bridge -Destination (Join-Path $stageDir "hardware-bridge\TaraTariHardwareBridge.exe") -Force
    Copy-Item -LiteralPath $bridge -Destination (Join-Path $portableDir "hardware-bridge\TaraTariHardwareBridge.exe") -Force
    Copy-Item -LiteralPath (Join-Path $root "README.md") -Destination $portableDir -Force
    Compress-Archive -Path (Join-Path $portableDir "*") `
        -DestinationPath (Join-Path $outputDir "TaraTariOptimizerApp-Portable-$($target.Name).zip") `
        -CompressionLevel Optimal -Force

    $setupPath = Join-Path $outputDir "TaraTariOptimizerApp-Setup-$($target.Name).msi"
    & $wix build $wxs -arch $target.Name -d "Arch=$($target.Name)" `
        -d "BuildDir=$stageDir" -o $setupPath
    if ($LASTEXITCODE -ne 0) {
        throw "WiX installer build failed for $($target.Name)."
    }
}

$sourceZip = Join-Path $outputDir "TaraTariOptimizerApp-Source.zip"
$ref = "v$Version"
$tags = & git -C $root tag --list $ref
if ($tags -contains $ref) {
    & git -C $root archive --format=zip --output=$sourceZip $ref
    if ($LASTEXITCODE -ne 0) {
        throw "Could not create source archive for $ref."
    }
}
else {
    Write-Warning "Source ZIP was not created because Git tag $ref does not exist yet."
}

Get-ChildItem $outputDir -File | Select-Object Name, Length
