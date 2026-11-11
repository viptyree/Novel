param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $repoRoot
try {
    if (-not $SkipBuild) {
        cargo build --locked --release -p novel-cli
        if ($LASTEXITCODE -ne 0) { throw 'CLI build failed' }
    }
    $binary = Join-Path $repoRoot 'target/release/novel.exe'
    $version = (& $binary --version).Split(' ')[1].Trim()
    $packageRoot = Join-Path $repoRoot 'target/packages'
    $staging = Join-Path $packageRoot ("stage-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path (Join-Path $staging 'docs') -Force | Out-Null
    Copy-Item -LiteralPath $binary -Destination $staging
    foreach ($name in @('README.md', 'README_EN.md', 'LICENSE')) {
        Copy-Item -LiteralPath (Join-Path $repoRoot $name) -Destination $staging
    }
    foreach ($name in @('CLI.md', '提示词设置使用说明.md', 'RELEASE-0.4.0.md')) {
        Copy-Item -LiteralPath (Join-Path $repoRoot "docs/$name") -Destination (Join-Path $staging 'docs')
    }
    $archive = Join-Path $packageRoot "novel-cli-$version-x86_64-pc-windows-msvc.zip"
    Compress-Archive -Path (Join-Path $staging '*') -DestinationPath $archive -Force
    Get-FileHash -LiteralPath $archive -Algorithm SHA256
    Write-Output $archive
} finally { Pop-Location }
