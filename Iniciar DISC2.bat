@echo off
cd /d "%~dp0"
if exist "DISC2.exe" (
  start "" "DISC2.exe"
) else if exist "target\release\disc2.exe" (
  start "" "target\release\disc2.exe"
) else (
  echo Executavel nao encontrado. Compile com: cargo build --release
  pause
)
