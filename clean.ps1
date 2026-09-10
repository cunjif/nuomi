# nuomi 清理脚本 / clean script
# 用法 / usage:
#   .\clean.ps1                  清理所有编译产物 (clean all build artifacts)
#   .\clean.ps1 -KeepTarget      保留 Rust target，只清前端与 Tauri 产物 (keep Rust target)
#   .\clean.ps1 -Deep            额外清理 node_modules，需重新 pnpm install (also remove node_modules)
#   .\clean.ps1 -Help            显示帮助 (show help)
#
# 清理目标 / targets:
#   target/                      Rust workspace 编译产物 (cargo clean)
#   dist/                        Vite 前端构建产物
#   src-tauri/gen/               Tauri 生成的 schema（dev/build 会重新生成）
#   node_modules/.vite/          Vite 预打包依赖缓存
#   node_modules/.cache/         通用工具缓存
#   **/*.tsbuildinfo             TypeScript 增量构建缓存
#   target/llvm-cov/             Rust 覆盖率报告（含在 target/ 内，cargo clean 一并清除）
param(
    [switch]$KeepTarget,
    [switch]$Deep,
    [switch]$Help
)

$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot

if ($Help) {
    Write-Host "nuomi clean — 清理编译产物 / clean build artifacts"
    Write-Host ""
    Write-Host "用法 / usage:"
    Write-Host "  .\clean.ps1                  清理所有编译产物"
    Write-Host "  .\clean.ps1 -KeepTarget      保留 Rust target，只清前端与 Tauri 产物"
    Write-Host "  .\clean.ps1 -Deep            额外清理 node_modules（需重新 pnpm install）"
    Write-Host "  .\clean.ps1 -Help            显示本帮助"
    exit 0
}

function Format-Size {
    param([long]$Bytes)
    if ($Bytes -le 0) { return "0 B" }
    $units = @("B", "KB", "MB", "GB", "TB")
    $i = 0
    $n = [double]$Bytes
    while ($n -ge 1024 -and $i -lt $units.Length - 1) {
        $n /= 1024
        $i++
    }
    if ($i -eq 0) { return "$([int]$n) $($units[$i])" }
    return "$([math]::Round($n, 1)) $($units[$i])"
}

function Get-DirSize {
    param([string]$Path)
    try {
        return (Get-ChildItem -Path $Path -Recurse -File -Force -ErrorAction SilentlyContinue |
            Measure-Object -Property Length -Sum).Sum
    } catch {
        return 0
    }
}

$script:freed = 0L
$script:removedCount = 0
$script:skippedCount = 0
# Any [err] sets this, so the exit code tells CI/callers cleanup was partial.
$script:hadError = $false

function Remove-Path {
    param([string]$Path)
    if (-not (Test-Path $Path)) {
        Write-Host "[skip] $Path — 不存在" -ForegroundColor DarkGray
        $script:skippedCount++
        return
    }
    $size = Get-DirSize -Path $Path
    try {
        Remove-Item -Path $Path -Recurse -Force -ErrorAction Stop
        $script:freed += $size
        $script:removedCount++
        Write-Host "[del]  $Path  ($(Format-Size $size))" -ForegroundColor Green
    } catch {
        Write-Host "[err]  $Path — $($_.Exception.Message)" -ForegroundColor Red
        $script:hadError = $true
    }
}

Write-Host ""
Write-Host "nuomi clean (KeepTarget=$KeepTarget, Deep=$Deep)" -ForegroundColor Cyan
Write-Host ""

# 1. Rust target/ — 优先 cargo clean（更彻底），无 cargo 则直接删目录
if (-not $KeepTarget) {
    if (Test-Path "target") {
        $size = Get-DirSize -Path "target"
        if (Get-Command cargo -ErrorAction SilentlyContinue) {
            # $ErrorActionPreference = "Stop" does NOT throw on a native command's
            # non-zero exit — only $LASTEXITCODE tells us cargo clean failed.
            # Reset it first so a stale value from an earlier command can't lie.
            $global:LASTEXITCODE = 0
            try {
                cargo clean 2>$null
                if ($LASTEXITCODE -ne 0) {
                    Write-Host "[warn] cargo clean exit $LASTEXITCODE, fallback to rmdir" -ForegroundColor Yellow
                    Remove-Path "target"
                } else {
                    $script:freed += $size
                    $script:removedCount++
                    Write-Host "[del]  target/  ($(Format-Size $size)) via cargo clean" -ForegroundColor Green
                }
            } catch {
                Remove-Path "target"
            }
        } else {
            Remove-Path "target"
        }
    } else {
        Write-Host "[skip] target/ — 不存在" -ForegroundColor DarkGray
        $script:skippedCount++
    }
} else {
    Write-Host "[keep] target/ — -KeepTarget" -ForegroundColor DarkGray
}

# 2. 前端构建产物
Remove-Path "dist"

# 3. Tauri 生成物（schema/capabilities，dev/build 会重新生成）
Remove-Path "src-tauri/gen"

# 4. 前端工具缓存
Remove-Path "node_modules/.vite"
Remove-Path "node_modules/.cache"

# 5. TypeScript 增量缓存（跳过 node_modules/target/.git 内部）
Get-ChildItem -Path . -Filter "*.tsbuildinfo" -Recurse -File -Force -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -notmatch '[\\/]node_modules[\\/]' -and $_.FullName -notmatch '[\\/]target[\\/]' -and $_.FullName -notmatch '[\\/]\.git[\\/]' } |
    ForEach-Object { Remove-Path $_.FullName }

# 6. 深度清理：node_modules
if ($Deep) {
    Remove-Path "node_modules"
}

Write-Host ""
$summary = "[done] 删除 $script:removedCount 项 / 跳过 $script:skippedCount 项 / 释放 $(Format-Size $script:freed)"
if ($script:hadError) {
    # Surface partial failures: green-only output would hide them from callers.
    Write-Host "$summary（有失败项，见上方 [err]）" -ForegroundColor Yellow
} else {
    Write-Host $summary -ForegroundColor Green
}
Write-Host ""

# Mirror start.ps1:51 — hand the real outcome to the caller/CI instead of
# leaving the exit code at whatever the last Write-Host produced (always 0).
exit ([int]$script:hadError)
