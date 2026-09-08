# Assertions for Remote.ps1. Run: pwsh -File Test-Remote.ps1
#
# Every case here goes through injected seams. Nothing in this suite starts, finds or stops a real
# process - which is the whole point: the defect under test is a function that killed a live server
# it had not identified, and a suite that reproduced it for real would do the same thing again.
try { . "$PSScriptRoot\..\claude-auto\Remote.ps1" } catch { Write-Host "COULD NOT RUN: $($_.Exception.Message)"; exit 2 }

$script:Failed = 0
$script:Ran = 0
function Assert-Equal {
    param($Expected, $Actual, [string]$Because)
    $script:Ran++
    if ("$Expected" -ne "$Actual") {
        Write-Host "FAIL  $Because"
        Write-Host "      expected: $Expected"
        Write-Host "      actual:   $Actual"
        $script:Failed++
    } else {
        Write-Host "ok    $Because"
    }
}

$root = 'C:\Users\someone\Projects\remote-control-claude-code'

# One recorded call per run, so "was it stopped" is answered by what the seam SAW, not by a message.
$script:Stopped = @()
$stopSeam = { param($ProcessId) $script:Stopped += $ProcessId }
function New-Listener { param([int]$ProcessId = 4242) { param($Port) [pscustomobject]@{ OwningProcess = $ProcessId } }.GetNewClosure() }
function New-ProcInfo {
    param([string]$Name, [string]$CommandLine)
    { param($ProcessId) [pscustomobject]@{ Name = $Name; CommandLine = $CommandLine } }.GetNewClosure()
}

# --- Nothing to stop -------------------------------------------------------------------------
$script:Stopped = @()
$r = Stop-CompanionServer -Root $root -GetListener { param($Port) $null } -StopProcess $stopSeam
Assert-Equal $true ($r -match 'nothing listening') 'an unheld port reports that nothing is listening'
Assert-Equal 0 $script:Stopped.Count 'and nothing is stopped'

# The socket table keeps a listen entry for a moment after the pid dies. Already gone is not a
# failure, and it is certainly not a reason to stop something else.
$script:Stopped = @()
$r = Stop-CompanionServer -Root $root -GetListener (New-Listener) -GetProcessInfo { param($ProcessId) $null } -StopProcess $stopSeam
Assert-Equal $true ($r -match 'stale socket entry') 'a listen entry for a dead pid reads as already stopped'
Assert-Equal 0 $script:Stopped.Count 'and nothing is stopped'

# --- The defect: a process on the port is not automatically ours -------------------------------
# 2026-09-08: this killed a live node server on 8791 that had nothing to do with the launcher.
$script:Stopped = @()
$r = Stop-CompanionServer -Root $root -GetListener (New-Listener -ProcessId 32620) `
    -GetProcessInfo (New-ProcInfo -Name 'node' -CommandLine 'node C:\Users\someone\Projects\something-else\server.js') `
    -StopProcess $stopSeam
Assert-Equal 0 $script:Stopped.Count 'a node process from another checkout is NOT stopped'
Assert-Equal $true ($r -match 'not the companion server') 'the message says it was left alone'
Assert-Equal $true ($r -match '32620') 'and names the pid, so the reader can decide themselves'

$script:Stopped = @()
$r = Stop-CompanionServer -Root $root -GetListener (New-Listener) `
    -GetProcessInfo (New-ProcInfo -Name 'python' -CommandLine "python serve.py $root") `
    -StopProcess $stopSeam
Assert-Equal 0 $script:Stopped.Count 'a non-node process holding the port is not stopped even from the right directory'
Assert-Equal $true ($r -match 'python') 'the message names what actually holds the port'

# A process whose command line cannot be read at all stays unidentified, and unidentified is safe.
$script:Stopped = @()
$r = Stop-CompanionServer -Root $root -GetListener (New-Listener) `
    -GetProcessInfo (New-ProcInfo -Name 'node' -CommandLine '') -StopProcess $stopSeam
Assert-Equal 0 $script:Stopped.Count 'an unreadable command line is not treated as a match'

# --- The one case that should stop -------------------------------------------------------------
$script:Stopped = @()
$r = Stop-CompanionServer -Root $root -GetListener (New-Listener -ProcessId 777) `
    -GetProcessInfo (New-ProcInfo -Name 'node' -CommandLine "node $root\server\dist\index.js") `
    -StopProcess $stopSeam
Assert-Equal 1 $script:Stopped.Count 'the real companion IS stopped'
Assert-Equal 777 $script:Stopped[0] 'and it is the pid that held the port'
Assert-Equal $true ($r -match 'stopped pid 777') 'the message reports it'

# The command line may carry either slash direction; the root check must not depend on which.
$script:Stopped = @()
$r = Stop-CompanionServer -Root $root -GetListener (New-Listener) `
    -GetProcessInfo (New-ProcInfo -Name 'node' -CommandLine 'node C:/Users/someone/Projects/remote-control-claude-code/server/dist/index.js') `
    -StopProcess $stopSeam
Assert-Equal 1 $script:Stopped.Count 'forward slashes in the command line still match the configured root'

# A failing stop is a status line, never a throw: this runs while the launcher is quitting.
$threw = $false
try {
    $r = Stop-CompanionServer -Root $root -GetListener (New-Listener) `
        -GetProcessInfo (New-ProcInfo -Name 'node' -CommandLine "node $root\server\dist\index.js") `
        -StopProcess { param($ProcessId) throw 'access is denied' }
} catch { $threw = $true }
Assert-Equal $false $threw 'a stop that fails does not throw out of the shutdown path'
Assert-Equal $true ($r -match 'access is denied') 'the reason reaches the caller'

# --- Root resolution ---------------------------------------------------------------------------
$savedRoot = $env:CLAUDE_REMOTE_ROOT
try {
    $env:CLAUDE_REMOTE_ROOT = 'D:\checkouts\crc'
    Assert-Equal 'D:\checkouts\crc' (Get-CompanionRoot) 'CLAUDE_REMOTE_ROOT wins when set'
    $env:CLAUDE_REMOTE_ROOT = ''
    Assert-Equal (Join-Path $HOME 'Desktop/Projects/remote-control-claude-code') (Get-CompanionRoot) 'otherwise the documented default'
} finally {
    if ($null -eq $savedRoot) { Remove-Item Env:CLAUDE_REMOTE_ROOT -ErrorAction SilentlyContinue }
    else { $env:CLAUDE_REMOTE_ROOT = $savedRoot }
}

if ($script:Ran -ne 18) { Write-Host "COULD NOT RUN: expected 18 assertions, ran $($script:Ran) - an assertion was skipped (its argument threw)"; exit 2 }
if ($script:Failed) { Write-Host ""; Write-Host "$script:Failed failed"; exit 1 }
Write-Host ""; Write-Host "all passed"
exit 0
