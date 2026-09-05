@echo off
rem nuomi 一键启动脚本 / one-click launcher
rem 用法 / usage:
rem   start.bat                默认启动 Tauri 桌面开发模式 (default: tauri desktop dev)
rem   start.bat cli "task"     启动 headless CLI (nuomi run)
rem   start.bat cli-resume id "msg"  续传会话 (nuomi resume)
rem   start.bat cli-repl       启动 headless CLI REPL
setlocal

cd /d "%~dp0"

where pnpm >nul 2>nul
if errorlevel 1 (
    echo [error] pnpm not found. Install Node.js >= 20 then run: corepack enable
    exit /b 1
)

if not exist "node_modules" (
    echo [setup] Installing frontend dependencies...
    call pnpm install
    if errorlevel 1 (
        echo [error] pnpm install failed.
        exit /b 1
    )
)

if /i "%~1"=="cli" (
    echo [nuomi] Starting headless CLI...
    cargo run -p nuomi-cli -- run %2 %3 %4
) else if /i "%~1"=="cli-resume" (
    echo [nuomi] Resuming CLI session...
    cargo run -p nuomi-cli -- resume %2 %3 %4
) else if /i "%~1"=="cli-repl" (
    echo [nuomi] Starting CLI REPL...
    cargo run -p nuomi-cli -- repl
) else (
    echo [nuomi] Starting Tauri desktop dev mode...
    call pnpm tauri dev
)

endlocal
