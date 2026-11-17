@echo off
cd /d "%~dp0"
if exist "ScreenZ.exe" (
  start "" "ScreenZ.exe"
) else if exist "target\release\screenz.exe" (
  start "" "target\release\screenz.exe"
) else (
  echo Executavel nao encontrado. Compile com: npm --prefix ScreenZWeb run desktop:build
  pause
)
