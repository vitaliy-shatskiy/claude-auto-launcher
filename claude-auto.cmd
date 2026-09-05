@echo off
REM Shim so `claude-auto` works from any shell, not just Rider's Claude Code plugin.
REM Forces pwsh: claude-auto.ps1 uses ForEach-Object -Parallel to find the Rider MCP port, which
REM Windows PowerShell 5.1 (still what `powershell` and Win+R resolve to) does not have. The .ps1
REM itself re-execs under pwsh when it is installed; without pwsh it falls back to the full UI with
REM no MCP configuration rather than dying - this shim exists so pwsh is used from the start.
REM Does not cd anywhere - the script reads Get-Location for the RC session name and
REM Claude itself opens the current directory as the project.
pwsh -ExecutionPolicy Bypass -File "%~dp0claude-auto.ps1" %*
exit /b %ERRORLEVEL%
