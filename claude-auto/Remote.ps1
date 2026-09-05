# Companion server and roaming support for the claude-auto launcher.
#
# Moved out of claude-auto.ps1 unchanged. The phone talks to the companion server, not to the
# session: birthing the session daemon-held is only half of "reachable from the phone" - without
# the server the app sees nothing at all. Started on demand rather than at logon, so the port is
# open while there is work and closed otherwise.

function Get-CompanionPort {
    $port = 8791
    $cfgPath = Join-Path $env:APPDATA 'claude-remote\config.json'
    if (Test-Path $cfgPath) {
        try {
            $p = (Get-Content $cfgPath -Raw | ConvertFrom-Json).port
            if ($p) { $port = [int]$p }
        } catch { }
    }
    return $port
}

function Stop-CompanionServer {
    # Nothing tracks a pid file, so "the server" is whatever listens on the configured port.
    # A foreign process squatting there would die too - acceptable on this machine, where the
    # port is reserved for the companion server.
    $port = Get-CompanionPort
    $conn = Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $conn) { return "nothing listening on $port" }
    # The socket table can briefly keep a listen entry for a pid that just died - treat a
    # missing process as already stopped, not as a failure.
    if (-not (Get-Process -Id $conn.OwningProcess -ErrorAction SilentlyContinue)) {
        return "nothing listening on $port (stale socket entry for pid $($conn.OwningProcess))"
    }
    try {
        Stop-Process -Id $conn.OwningProcess -Force -ErrorAction Stop
        return "stopped pid $($conn.OwningProcess) on port $port"
    } catch {
        return "failed to stop pid $($conn.OwningProcess): $($_.Exception.Message)"
    }
}

function Start-CompanionServer {
    param([string]$Root)
    $serverDir = Join-Path $Root 'server'
    if (-not (Test-Path (Join-Path $serverDir 'package.json'))) { return "no checkout at $Root" }
    $port = Get-CompanionPort
    if (Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue) {
        return "already listening on $port"
    }
    $log = Join-Path $env:TEMP 'claude-remote-server.log'

    # Production build first: `npm run build` emits dist/, node runs it without tsx. Rebuild only
    # when a source/config file is newer than dist/index.js. A checkout without the build script
    # (pre-merge) or a failed build falls back to the old tsx path - a broken build must never
    # leave the phone without a server, and the fallback is exactly what ran before.
    $mode = 'tsx'
    try {
        $dist = Join-Path $serverDir 'dist\index.js'
        $pkg = Get-Content (Join-Path $serverDir 'package.json') -Raw | ConvertFrom-Json
        if ($pkg.scripts.build) {
            $newest = (Get-ChildItem (Join-Path $serverDir 'src') -Recurse -Filter *.ts |
                Measure-Object -Property LastWriteTimeUtc -Maximum).Maximum
            foreach ($extra in @('package.json', 'tsconfig.json', 'tsconfig.build.json')) {
                $f = Join-Path $serverDir $extra
                if ((Test-Path $f) -and ((Get-Item $f).LastWriteTimeUtc -gt $newest)) { $newest = (Get-Item $f).LastWriteTimeUtc }
            }
            $stale = -not (Test-Path $dist) -or ((Get-Item $dist).LastWriteTimeUtc -lt $newest)
            if ($stale) {
                Push-Location $serverDir
                try { & npm run build --silent *> "$log.build" } finally { Pop-Location }
                if ($LASTEXITCODE -eq 0 -and (Test-Path $dist)) { $mode = 'dist' }
                else { Write-Host "  server build failed - falling back to tsx (see $log.build)" -ForegroundColor DarkYellow }
            } else {
                $mode = 'dist'
            }
        }
    } catch {
        Write-Host "  build check failed - falling back to tsx: $($_.Exception.Message)" -ForegroundColor DarkYellow
    }

    $serverArgs = if ($mode -eq 'dist') { @('dist/index.js') } else { @('--import', 'tsx', 'src/index.ts') }
    Start-Process -FilePath 'node' -ArgumentList $serverArgs `
        -WorkingDirectory $serverDir `
        -RedirectStandardOutput $log -RedirectStandardError "$log.err" `
        -WindowStyle Hidden | Out-Null
    # Report what actually happened, not what was intended: a server that failed to bind must not
    # read as a working one, or the phone silently shows nothing and the cause looks like the app.
    for ($i = 0; $i -lt 24; $i++) {
        Start-Sleep -Milliseconds 250
        if (Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue) {
            return "started on $port ($mode)"
        }
    }
    return "did not come up within 6s - see $log"
}

function Show-PairingQr {
    # The QR carries the address and the bearer token, so it is printed only on request and
    # only here - Claude takes the whole terminal over a second later, and the codes scroll
    # away with it. Waiting for a keypress is the point: an unscanned QR is a wasted prompt.
    param([string]$Root)
    try {
        Push-Location (Join-Path $Root 'server')
        try { & npm run pair --silent } finally { Pop-Location }
        # `npm run pair` refuses and exits non-zero when it cannot verify that the address
        # in the QR answers with this token - it prints why and no code. Do not then ask
        # for a scan: there is nothing on screen to scan, and the pause would only delay
        # Claude while hiding the reason.
        if ($LASTEXITCODE -eq 0) {
            Write-Host "Scan it, then press Enter to start Claude : " -NoNewline -ForegroundColor Cyan
            [void](Read-Host)
        } else {
            Write-Host "  no QR printed (see the message above); starting Claude anyway" -ForegroundColor DarkYellow
        }
    } catch {
        Write-Host "  QR not shown: $($_.Exception.Message)" -ForegroundColor DarkYellow
    }
}
