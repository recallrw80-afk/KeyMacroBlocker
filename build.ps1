# 用 cargo 构建（无第三方依赖，离线也能编）
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
if (-not (Test-Path $cargo)) { $cargo = 'cargo' }

Push-Location $root
try {
    & $cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "编译失败 ($LASTEXITCODE)" }
} finally {
    Pop-Location
}

$out = Join-Path $root 'bin'
New-Item -ItemType Directory -Force -Path $out | Out-Null
Copy-Item (Join-Path $root 'target\release\KeyMacroBlocker.exe') $out -Force
Copy-Item (Join-Path $root 'patterns.txt') $out -Force
Write-Host "已生成: $(Join-Path $out 'KeyMacroBlocker.exe')" -ForegroundColor Green
