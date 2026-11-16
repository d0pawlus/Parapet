@echo off
setlocal
set VCVARS=C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\VC\Auxiliary\Build\vcvars64.bat
call "%VCVARS%" >nul || exit /b 1
cd /d "%~dp0"
set OUT=bin\Parapet.exe
set DEFS=
if /i "%1"=="debug" (set OUT=bin\Parapet-debug.exe& set DEFS=/DOVERLAY_DEBUG)
if not exist bin mkdir bin
if not exist obj mkdir obj
cl /nologo /std:c++17 /O2 /GL /GS- /Gy /MT /EHsc /W3 /utf-8 /DUNICODE /D_UNICODE %DEFS% /Foobj\ /Fe:%OUT% ^
   src\main.cpp src\sim.cpp src\tracker.cpp src\render.cpp src\weather.cpp src\leaves.cpp src\critters.cpp ^
   /link /LTCG /OPT:REF /OPT:ICF /SUBSYSTEM:WINDOWS ^
   user32.lib gdi32.lib shell32.lib advapi32.lib dwmapi.lib d3d11.lib dxgi.lib d2d1.lib dcomp.lib winhttp.lib wtsapi32.lib
