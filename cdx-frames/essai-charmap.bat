@echo off
rem Ouvre la Table des caractères et lui met le cadre CDX ; Ctrl+C dans cette fenêtre remet tout comme avant.
start "" charmap.exe
timeout /t 2 /nobreak >nul
"%~dp0target\release\cdx-frames.exe" --console charmap.exe
