# nuomi 一键启动脚本 / one-click launcher
# 用法 / usage:
#   .\start.ps1                      默认启动 Tauri 桌面开发模式 (default: tauri desktop dev)
#   .\start.ps1 cli "任务"           启动 headless CLI 执行任务 (nuomi-cli run)
#   .\start.ps1 cli-resume <id> "…"  续传会话 (nuomi-cli resume)
#   .\start.ps1 cli-repl             启动 headless CLI REPL (nuomi-cli repl)
param(
    [Parameter(Position = 0)]
    [string]$Mode = "dev",

    [Parameter(Position = 1, ValueFromRemainingArguments = $true)]
    [string[]]$CliArgs
)

$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot

if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
    Write-Host "[error] pnpm not found. Install Node.js >= 20 then run: corepack enable" -ForegroundColor Red
    exit 1
}

if (-not (Test-Path "node_modules")) {
    Write-Host "[setup] Installing frontend dependencies..."
    pnpm install
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[error] pnpm install failed." -ForegroundColor Red
        exit 1
    }
}

switch ($Mode.ToLower()) {
    "cli" {
        Write-Host "[nuomi] Starting headless CLI..."
        cargo run -p nuomi-cli -- run @CliArgs
    }
    "cli-resume" {
        Write-Host "[nuomi] Resuming CLI session..."
        cargo run -p nuomi-cli -- resume @CliArgs
    }
    "cli-repl" {
        Write-Host "[nuomi] Starting CLI REPL..."
        cargo run -p nuomi-cli -- repl
    }
    default {
        Write-Host "[nuomi] Starting Tauri desktop dev mode..."
        pnpm tauri dev
    }
}

exit $LASTEXITCODE
