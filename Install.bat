@echo off
rem Makes the Ratchet Movement mod from your own Rift Apart install (see INSTALL.md).
rem Any options go through to install.ps1, e.g.  Install.bat -Zip
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
echo.
pause
