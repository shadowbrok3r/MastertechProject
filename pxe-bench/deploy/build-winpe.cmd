@echo off
setlocal EnableExtensions
title Mastertech WinPE builder

rem ===== settings (edit if your paths differ) =====
set "ADK=C:\Program Files (x86)\Windows Kits\10\Assessment and Deployment Kit"
set "SRCWIM=%ADK%\Windows Preinstallation Environment\amd64\en-us\winpe.wim"
set "OCS=%ADK%\Windows Preinstallation Environment\amd64\WinPE_OCs"
set "MT=C:\ct\release-fast\MasterTech.exe"
set "WORK=C:\WinPE_MT"
rem PXE server to deploy to (scp). PXEROOT must equal http_root in that server's pxe-bench.toml.
set "PXEHOST=shadowbroker@192.168.22.15"
set "PXEROOT=/srv/pxe/http"
set "PESRC=%~dp0winpe"
set "DISM=%SystemRoot%\System32\Dism.exe"

rem ===== must be elevated (DISM image-mount is admin-only) =====
net session >nul 2>&1
if errorlevel 1 ( echo [X] Run this from an ELEVATED prompt ^(Run as administrator^). & pause & exit /b 1 )

rem ===== sanity (no ()-blocks: an expanded "(x86)" path breaks cmd block parsing) =====
if not exist "%SRCWIM%" echo [X] base winpe.wim missing: %SRCWIM%
if not exist "%SRCWIM%" exit /b 1
if not exist "%MT%" echo [X] MasterTech.exe missing: %MT%
if not exist "%MT%" exit /b 1
if not exist "%OCS%\WinPE-WMI.cab" echo [X] WinPE_OCs missing: %OCS%
if not exist "%OCS%\WinPE-WMI.cab" exit /b 1
if not exist "%PESRC%\startnet.cmd" echo [X] PE startup file missing: %PESRC%\startnet.cmd
if not exist "%PESRC%\startnet.cmd" exit /b 1

rem ===== clean any prior run =====
if exist "%WORK%\mount\Windows" ( echo [*] discarding stale mount & "%DISM%" /Unmount-Image /MountDir:"%WORK%\mount" /Discard >nul 2>&1 )
"%DISM%" /Cleanup-Wim >nul 2>&1
if exist "%WORK%" rd /s /q "%WORK%"
mkdir "%WORK%\mount"
mkdir "%WORK%\media\sources"

rem ===== stage + mount the stock WinPE image as boot.wim =====
echo [*] copying base image (336 MB)...
copy /y "%SRCWIM%" "%WORK%\media\sources\boot.wim" >nul || ( echo [X] copy base wim failed & exit /b 1 )
attrib -r "%WORK%\media\sources\boot.wim"
echo [*] mounting boot.wim...
"%DISM%" /Mount-Image /ImageFile:"%WORK%\media\sources\boot.wim" /Index:1 /MountDir:"%WORK%\mount" || ( echo [X] mount failed & exit /b 1 )

rem ===== optional components: dependency order, neutral then en-us =====
echo [*] WinPE-WMI
"%DISM%" /Image:"%WORK%\mount" /Add-Package /PackagePath:"%OCS%\WinPE-WMI.cab" || goto :fail
"%DISM%" /Image:"%WORK%\mount" /Add-Package /PackagePath:"%OCS%\en-us\WinPE-WMI_en-us.cab" || goto :fail
echo [*] WinPE-NetFx
"%DISM%" /Image:"%WORK%\mount" /Add-Package /PackagePath:"%OCS%\WinPE-NetFx.cab" || goto :fail
"%DISM%" /Image:"%WORK%\mount" /Add-Package /PackagePath:"%OCS%\en-us\WinPE-NetFx_en-us.cab" || goto :fail
echo [*] WinPE-Scripting
"%DISM%" /Image:"%WORK%\mount" /Add-Package /PackagePath:"%OCS%\WinPE-Scripting.cab" || goto :fail
"%DISM%" /Image:"%WORK%\mount" /Add-Package /PackagePath:"%OCS%\en-us\WinPE-Scripting_en-us.cab" || goto :fail
echo [*] WinPE-PowerShell
"%DISM%" /Image:"%WORK%\mount" /Add-Package /PackagePath:"%OCS%\WinPE-PowerShell.cab" || goto :fail
"%DISM%" /Image:"%WORK%\mount" /Add-Package /PackagePath:"%OCS%\en-us\WinPE-PowerShell_en-us.cab" || goto :fail
echo [*] WinPE-StorageWMI
"%DISM%" /Image:"%WORK%\mount" /Add-Package /PackagePath:"%OCS%\WinPE-StorageWMI.cab" || goto :fail
"%DISM%" /Image:"%WORK%\mount" /Add-Package /PackagePath:"%OCS%\en-us\WinPE-StorageWMI_en-us.cab" || goto :fail

rem ===== VC++ runtime + fetch-at-boot startup (client is NOT baked in) =====
echo [*] injecting VC++ runtime + boot-fetch startup...
copy /y "%SystemRoot%\System32\vcruntime140.dll"   "%WORK%\mount\Windows\System32\" >nul
copy /y "%SystemRoot%\System32\vcruntime140_1.dll" "%WORK%\mount\Windows\System32\" >nul
copy /y "%SystemRoot%\System32\msvcp140.dll"        "%WORK%\mount\Windows\System32\" >nul
rem MasterTech.exe statically imports these; stock WinPE lacks them, so the loader
rem silently refuses to start the exe. Confirmed load-time deps via dumpbin /dependents.
copy /y "%SystemRoot%\System32\opengl32.dll" "%WORK%\mount\Windows\System32\" >nul || goto :fail
copy /y "%SystemRoot%\System32\pdh.dll"      "%WORK%\mount\Windows\System32\" >nul || goto :fail
copy /y "%SystemRoot%\System32\wlanapi.dll"  "%WORK%\mount\Windows\System32\" >nul || goto :fail
copy /y "%PESRC%\startnet.cmd" "%WORK%\mount\Windows\System32\startnet.cmd" >nul || goto :fail

rem ===== inject NIC/other drivers from deploy\drivers\ if that folder exists =====
set "DRIVERS=%~dp0drivers"
if exist "%DRIVERS%\." echo [*] injecting drivers from %DRIVERS% ...
if exist "%DRIVERS%\." "%DISM%" /Image:"%WORK%\mount" /Add-Driver /Driver:"%DRIVERS%" /Recurse /ForceUnsigned

rem ===== commit =====
echo [*] committing (unmount /commit, ~1-2 min)...
"%DISM%" /Unmount-Image /MountDir:"%WORK%\mount" /Commit || ( echo [X] commit failed & exit /b 1 )

for %%F in ("%WORK%\media\sources\boot.wim") do set "WIMSZ=%%~zF"
echo.
echo [OK] boot.wim built: %WORK%\media\sources\boot.wim  (%WIMSZ% bytes)

rem ===== deploy boot.wim + client to the PXE server via scp =====
echo [*] deploying to %PXEHOST%:%PXEROOT% via scp (enter password if prompted)...
scp "%WORK%\media\sources\boot.wim" "%PXEHOST%:%PXEROOT%/media/sources/boot.wim" && echo [OK] boot.wim deployed || echo [!] scp boot.wim failed
scp "%MT%" "%PXEHOST%:%PXEROOT%/MasterTech.exe" && echo [OK] client deployed || echo [!] scp client failed
echo.
echo If scp failed above, run these from a normal (non-elevated) terminal:
echo   scp "%WORK%\media\sources\boot.wim" %PXEHOST%:%PXEROOT%/media/sources/boot.wim
echo   scp "%MT%" %PXEHOST%:%PXEROOT%/MasterTech.exe
echo.
echo Then on the PXE server (%PXEHOST%) verify + re-netboot:
echo   curl -sI http://192.168.22.15:7777/media/sources/boot.wim
exit /b 0

:fail
echo [X] build failed - discarding mount
"%DISM%" /Unmount-Image /MountDir:"%WORK%\mount" /Discard >nul 2>&1
exit /b 1
