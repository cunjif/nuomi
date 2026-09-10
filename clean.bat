@echo off
rem nuomi 清理脚本 / clean script
rem 用法 / usage:
rem   clean.bat                  清理所有编译产物 (clean all build artifacts)
rem   clean.bat -KeepTarget      保留 Rust target，只清前端与 Tauri 产物 (keep Rust target)
rem   clean.bat -Deep            额外清理 node_modules，需重新 pnpm install (also remove node_modules)
rem   clean.bat -Help            显示帮助 (show help)
setlocal

cd /d "%~dp0"

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0clean.ps1" %*
rem Capture before endlocal: `endlocal & exit /b` is the idiom that carries
rem the exit code out of the setlocal scope (clean.ps1 exits 1 on any [err]).
set EXITCODE=%ERRORLEVEL%

endlocal & exit /b %EXITCODE%
