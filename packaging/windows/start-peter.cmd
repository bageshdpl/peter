@echo off
title Peter
rem Starts Peter from this folder. No installation or admin rights needed.
cd /d "%~dp0"

set "PETER_BIND=127.0.0.1:5173"
set "PETER_APP_URL=http://localhost:5173"
set "PETER_WEB_DIR=%~dp0web"

rem Your AI keys, if you made my-keys.cmd (see my-keys.example.cmd).
if exist "%~dp0my-keys.cmd" call "%~dp0my-keys.cmd"

rem Open the browser a few seconds after the server starts.
start "" /b cmd /c "ping -n 4 127.0.0.1 >nul & start http://localhost:5173"

echo.
echo   Peter is starting. Your browser will open at http://localhost:5173
echo   Keep this window open while you use Peter. Close it to stop Peter.
echo.
peter-server.exe
echo.
echo   Peter has stopped.
pause
