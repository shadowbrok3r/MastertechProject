@echo off
setlocal EnableExtensions
title Mastertech client deploy

rem Push a freshly-built MasterTech.exe to the PXE server so the next netboot
rem fetches it. No WinPE rebuild, no elevation needed (just a share copy).
set "MT=C:\ct\release-fast\MasterTech.exe"
set "HTTPROOT=\\192.168.22.7\Mtech\pxe\http"

if not exist "%MT%" echo [X] MasterTech.exe missing: %MT%
if not exist "%MT%" exit /b 1
if not exist "%HTTPROOT%" echo [X] PXE server share not reachable: %HTTPROOT%
if not exist "%HTTPROOT%" exit /b 1

echo [*] deploying %MT%
echo         -^> %HTTPROOT%\MasterTech.exe
copy /y "%MT%" "%HTTPROOT%\MasterTech.exe" >nul || ( echo [X] copy failed & exit /b 1 )
for %%F in ("%MT%") do echo [OK] deployed (%%~zF bytes). Next PXE boot fetches the new client - no rebuild needed.
exit /b 0
