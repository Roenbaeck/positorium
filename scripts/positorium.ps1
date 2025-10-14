<#!
.SYNOPSIS
  Convenience functions to start/stop the Positorium server with logging configuration.
.DESCRIPTION
  Provides Start-Positorium / Stop-Positorium and a lightweight restart helper.
  Adds an alias 'positorium-run' for quick invocation in the current terminal.
  You can dot-source this file ( . .\scripts\positorium.ps1 ) to load the functions
  into your session, or just execute the script to start immediately.
.PARAMETER Log
  Comma separated list of tracing directives (passed to RUST_LOG / Positorium
  tracing subscriber via EnvFilter). Examples:
    'info'                   -> info and above for all modules
    'warn,positorium=info'   -> global warn, crate-specific info
    'trace,axum=info'        -> very verbose except axum trimmed
.PARAMETER LogProfile
  Shortcut presets for common log configurations: quiet | normal | verbose | trace
.EXAMPLE
  # Start with default (normal) logging
  Start-Positorium
.EXAMPLE
  # Start with custom tracing directives
  Start-Positorium -Log 'warn,positorium=info'
.EXAMPLE
  # Restart quickly with verbose logging
  Restart-Positorium -LogProfile verbose
#>

[CmdletBinding()] param(
    [string] $AutoStart = 'normal'
)

$script:POSITORIUM_PROCESS = $null
$script:POSITORIUM_REPO_ROOT = try { Split-Path $PSScriptRoot -Parent } catch { (Resolve-Path '..').Path }

function Set-PositoriumLogEnv {
    param(
        [Parameter(Mandatory=$false)][string] $Log,
        [ValidateSet('quiet','normal','verbose','trace')][string] $LogProfile = 'normal'
    )
    if (-not $Log) {
        switch ($LogProfile) {
            'quiet'   { $Log = 'error' }
            'normal'  { $Log = 'info' }
            'verbose' { $Log = 'debug,positorium=info' }
            'trace'   { $Log = 'trace' }
        }
    }
    $env:RUST_LOG = $Log
    Write-Host "[positorium] RUST_LOG set to '$Log'" -ForegroundColor Cyan
}

function Start-Positorium {
    [CmdletBinding()] param(
        [string] $Log,
        [ValidateSet('quiet','normal','verbose','trace')][string] $LogProfile = 'normal',
        [switch] $ForceRebuild,
        [switch] $Release,
        [switch] $Tail
    )
    if ($script:POSITORIUM_PROCESS -and -not $script:POSITORIUM_PROCESS.HasExited) {
        Write-Warning 'Positorium already running. Use Restart-Positorium or Stop-Positorium first.'
        return
    }
    Set-PositoriumLogEnv -Log $Log -LogProfile $LogProfile
    $cargoArgs = @('run','--quiet')
    if ($Release) { $cargoArgs = @('run','--release','--quiet') }
    if ($ForceRebuild) { Write-Host '[positorium] Forcing clean build...' -ForegroundColor Yellow; cargo clean | Out-Null }
    Write-Host "[positorium] Starting (args: $($cargoArgs -join ' '))..." -ForegroundColor Green
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = 'cargo'
    # Ensure we always point cargo at the repository root (parent of the scripts folder)
    $psi.WorkingDirectory = $script:POSITORIUM_REPO_ROOT
    $psi.UseShellExecute = $false
    if ($Tail) {
        # Stream logs directly to this console
        $psi.RedirectStandardOutput = $false
        $psi.RedirectStandardError = $false
    } else {
        # Capture (suppresses live log display)
        $psi.RedirectStandardOutput = $true
        $psi.RedirectStandardError = $true
    }
    # Build a single argument string instead of using ArgumentList (read-only / not settable on some Windows PowerShell versions)
    $psi.Arguments = ($cargoArgs -join ' ')
    $proc = New-Object System.Diagnostics.Process
    $proc.StartInfo = $psi
    $null = $proc.Start()
    $script:POSITORIUM_PROCESS = $proc
    if (-not $Tail) {
        # NOTE: Output captured silently. Use -Tail to see logs live. Potential enhancement: background reader.
    }
    Start-Sleep -Milliseconds 600
    if ($proc.HasExited) {
        Write-Warning "[positorium] Process exited early with code $($proc.ExitCode)"
        Write-Host ($proc.StandardError.ReadToEnd())
        return
    }
    Write-Host "[positorium] Running (PID: $($proc.Id))" -ForegroundColor Green
}

function Stop-Positorium {
    [CmdletBinding()] param()
    if (-not $script:POSITORIUM_PROCESS) { Write-Host '[positorium] Not started in this session.'; return }
    if ($script:POSITORIUM_PROCESS.HasExited) { Write-Host '[positorium] Already exited.'; return }
    Write-Host "[positorium] Stopping PID $($script:POSITORIUM_PROCESS.Id)..." -ForegroundColor Yellow
    try {
        $script:POSITORIUM_PROCESS.Kill()
        $script:POSITORIUM_PROCESS.WaitForExit(3000) | Out-Null
    } catch {
        Write-Warning "[positorium] Failed to kill process: $_"
    }
}

function Restart-Positorium {
    [CmdletBinding()] param(
        [string] $Log,
        [ValidateSet('quiet','normal','verbose','trace')][string] $LogProfile = 'normal',
        [switch] $ForceRebuild,
        [switch] $Release
    )
    Stop-Positorium
    Start-Positorium -Log $Log -LogProfile $LogProfile -ForceRebuild:$ForceRebuild -Release:$Release
}

Set-Alias positorium-run Start-Positorium

if ($MyInvocation.InvocationName -ne '.') {
    # If script is executed directly, start with provided AutoStart profile
    Start-Positorium -LogProfile $AutoStart
}

