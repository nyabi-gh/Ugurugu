# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)

[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string]$PackageRoot,

    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string]$SmokeExecutable,

    [ValidateRange(1, 60)]
    [int]$TimeoutSeconds = 15,

    [ValidateRange(1, 30)]
    [int]$StartupSeconds = 5
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Copy-PackageForSmoke {
    param(
        [string]$Source,
        [string]$Destination,
        [string]$Probe
    )

    New-Item -ItemType Directory -Path $Destination | Out-Null
    Get-ChildItem -LiteralPath $Source -Force | ForEach-Object {
        Copy-Item `
            -LiteralPath $_.FullName `
            -Destination $Destination `
            -Recurse `
            -Force
    }
    $probeDestination = Join-Path $Destination 'ugurugu_package_smoke.exe'
    Copy-Item -LiteralPath $Probe -Destination $probeDestination
    return $probeDestination
}

function New-IsolatedStartInfo {
    param(
        [string]$Executable,
        [string]$WorkingDirectory,
        [string]$Argument = ''
    )

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $Executable
    $startInfo.WorkingDirectory = $WorkingDirectory
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    if ($Argument) {
        $startInfo.Arguments = '"' + $Argument + '"'
    }
    $startInfo.EnvironmentVariables.Clear()

    foreach ($entry in [System.Environment]::GetEnvironmentVariables().GetEnumerator()) {
        $name = [string]$entry.Key
        if ($name -ne 'PATH' -and
            $name -ne 'CMAKE_PREFIX_PATH' -and
            $name -notmatch '^(QT|QML)') {
            $startInfo.EnvironmentVariables[$name] = [string]$entry.Value
        }
    }

    $windowsRoot = [System.Environment]::GetEnvironmentVariable('SystemRoot')
    if ([string]::IsNullOrWhiteSpace($windowsRoot)) {
        throw 'SystemRoot is not defined.'
    }
    $startInfo.EnvironmentVariables['PATH'] = @(
        (Join-Path $windowsRoot 'System32')
        $windowsRoot
        (Join-Path $windowsRoot 'System32\Wbem')
    ) -join [System.IO.Path]::PathSeparator
    $profileRoot = Join-Path $WorkingDirectory '.smoke-profile'
    $roamingProfile = Join-Path $profileRoot 'Roaming'
    $localProfile = Join-Path $profileRoot 'Local'
    New-Item -ItemType Directory -Path $roamingProfile -Force | Out-Null
    New-Item -ItemType Directory -Path $localProfile -Force | Out-Null
    $startInfo.EnvironmentVariables['APPDATA'] = $roamingProfile
    $startInfo.EnvironmentVariables['LOCALAPPDATA'] = $localProfile
    $startInfo.EnvironmentVariables['USERPROFILE'] = $profileRoot
    $startInfo.EnvironmentVariables['UGURUGU_RECOVERY_PATH'] =
        Join-Path $profileRoot 'recovery.ugu'
    return $startInfo
}

function Invoke-PackageSmoke {
    param(
        [string]$Executable,
        [string]$WorkingDirectory
    )

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = New-IsolatedStartInfo `
        $Executable `
        $WorkingDirectory `
        $WorkingDirectory
    try {
        if (-not $process.Start()) {
            return [pscustomobject]@{
                StartError = 'Process.Start returned false.'
                TimedOut = $false
                ExitCode = $null
                StandardOutput = ''
                StandardError = ''
            }
        }
    }
    catch {
        return [pscustomobject]@{
            StartError = $_.Exception.Message
            TimedOut = $false
            ExitCode = $null
            StandardOutput = ''
            StandardError = ''
        }
    }

    $standardOutput = $process.StandardOutput.ReadToEndAsync()
    $standardError = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        $process.Kill()
        $process.WaitForExit()
        return [pscustomobject]@{
            StartError = $null
            TimedOut = $true
            ExitCode = $process.ExitCode
            StandardOutput = $standardOutput.GetAwaiter().GetResult()
            StandardError = $standardError.GetAwaiter().GetResult()
        }
    }
    return [pscustomobject]@{
        StartError = $null
        TimedOut = $false
        ExitCode = $process.ExitCode
        StandardOutput = $standardOutput.GetAwaiter().GetResult()
        StandardError = $standardError.GetAwaiter().GetResult()
    }
}

function Assert-ApplicationStarts {
    param(
        [string]$Executable,
        [string]$WorkingDirectory
    )

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = New-IsolatedStartInfo $Executable $WorkingDirectory
    try {
        if (-not $process.Start()) {
            throw 'Process.Start returned false.'
        }
    }
    catch {
        throw "The installed application did not start: $($_.Exception.Message)"
    }

    $standardOutput = $process.StandardOutput.ReadToEndAsync()
    $standardError = $process.StandardError.ReadToEndAsync()
    if ($process.WaitForExit($StartupSeconds * 1000)) {
        $result = [pscustomobject]@{
            StartError = $null
            TimedOut = $false
            ExitCode = $process.ExitCode
            StandardOutput = $standardOutput.GetAwaiter().GetResult()
            StandardError = $standardError.GetAwaiter().GetResult()
        }
        throw "The installed application exited during startup.`n$(Format-SmokeResult $result)"
    }
    $process.Kill()
    $process.WaitForExit()
    $standardOutput.GetAwaiter().GetResult() | Out-Null
    $standardError.GetAwaiter().GetResult() | Out-Null
}

function Format-SmokeResult {
    param([pscustomobject]$Result)

    return @(
        "start error: $($Result.StartError)"
        "timed out: $($Result.TimedOut)"
        "exit code: $($Result.ExitCode)"
        "stdout: $($Result.StandardOutput)"
        "stderr: $($Result.StandardError)"
    ) -join [System.Environment]::NewLine
}

function Assert-SmokeSucceeded {
    param([pscustomobject]$Result)

    if ($Result.StartError -or $Result.TimedOut -or $Result.ExitCode -ne 0) {
        throw "The isolated package smoke failed.`n$(Format-SmokeResult $Result)"
    }
}

function Assert-SmokeFailed {
    param(
        [pscustomobject]$Result,
        [string]$ControlName
    )

    if ($Result.TimedOut) {
        throw "The $ControlName control hung instead of failing.`n$(Format-SmokeResult $Result)"
    }
    if (-not $Result.StartError -and $Result.ExitCode -eq 0) {
        throw "The $ControlName control unexpectedly passed.`n$(Format-SmokeResult $Result)"
    }
}

function Remove-ScratchRoot {
    param(
        [string]$Root,
        [string]$Base
    )

    $resolvedRoot = [System.IO.Path]::GetFullPath($Root).TrimEnd('\', '/')
    $resolvedBase = [System.IO.Path]::GetFullPath($Base).TrimEnd('\', '/')
    $expectedPrefix = $resolvedBase + [System.IO.Path]::DirectorySeparatorChar
    $leaf = [System.IO.Path]::GetFileName($resolvedRoot)
    if (-not $resolvedRoot.StartsWith(
            $expectedPrefix,
            [System.StringComparison]::OrdinalIgnoreCase) -or
        $leaf -notmatch '^Ugurugu-PackageSmoke-[0-9a-f-]{36}$') {
        throw "Refusing to remove unexpected smoke directory: $resolvedRoot"
    }
    if (Test-Path -LiteralPath $resolvedRoot) {
        Remove-Item -LiteralPath $resolvedRoot -Recurse -Force
    }
}

$resolvedPackageRoot = (Resolve-Path -LiteralPath $PackageRoot).Path
if (-not (Get-Item -LiteralPath $resolvedPackageRoot).PSIsContainer) {
    throw "The package root is not a directory: $resolvedPackageRoot"
}
$resolvedSmokeExecutable = (Resolve-Path -LiteralPath $SmokeExecutable).Path
if ((Get-Item -LiteralPath $resolvedSmokeExecutable).PSIsContainer) {
    throw "The smoke executable is not a file: $resolvedSmokeExecutable"
}

$requiredFiles = @(
    'Ugurugu.exe'
    'Qt6Core.dll'
    'Qt6Gui.dll'
    'Qt6Widgets.dll'
    'platforms\qwindows.dll'
    'imageformats\qjpeg.dll'
    'qt.conf'
    'velopack_libc.dll'
    'LICENSE'
    'README.md'
    'THIRD_PARTY_NOTICES.md'
    'Velopack-LICENSE.txt'
    'zlib-LICENSE.txt'
)
foreach ($relativePath in $requiredFiles) {
    $requiredPath = Join-Path $resolvedPackageRoot $relativePath
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
        throw "The installed package is missing $relativePath."
    }
}

$qtConfiguration = Get-Content `
    -LiteralPath (Join-Path $resolvedPackageRoot 'qt.conf') `
    -Raw
if ($qtConfiguration -notmatch '(?im)^\s*Plugins\s*=\s*\.\s*$') {
    throw 'qt.conf does not restrict plugins to the application directory.'
}

$scratchBase = if ($env:RUNNER_TEMP) {
    $env:RUNNER_TEMP
}
else {
    [System.IO.Path]::GetTempPath()
}
$scratchRoot = Join-Path `
    $scratchBase `
    "Ugurugu-PackageSmoke-$([guid]::NewGuid())"
New-Item -ItemType Directory -Path $scratchRoot | Out-Null

try {
    $positiveRoot = Join-Path $scratchRoot 'complete'
    $positiveProbe = Copy-PackageForSmoke `
        $resolvedPackageRoot `
        $positiveRoot `
        $resolvedSmokeExecutable
    Assert-SmokeSucceeded (Invoke-PackageSmoke $positiveProbe $positiveRoot)
    Assert-ApplicationStarts `
        (Join-Path $positiveRoot 'Ugurugu.exe') `
        $positiveRoot

    $missingPluginRoot = Join-Path $scratchRoot 'missing-platform-plugin'
    $missingPluginProbe = Copy-PackageForSmoke `
        $resolvedPackageRoot `
        $missingPluginRoot `
        $resolvedSmokeExecutable
    Remove-Item `
        -LiteralPath (Join-Path $missingPluginRoot 'platforms\qwindows.dll')
    Assert-SmokeFailed `
        (Invoke-PackageSmoke $missingPluginProbe $missingPluginRoot) `
        'missing platform plugin'

    $missingRuntimeRoot = Join-Path $scratchRoot 'missing-Qt-runtime'
    $missingRuntimeProbe = Copy-PackageForSmoke `
        $resolvedPackageRoot `
        $missingRuntimeRoot `
        $resolvedSmokeExecutable
    Remove-Item -LiteralPath (Join-Path $missingRuntimeRoot 'Qt6Core.dll')
    Assert-SmokeFailed `
        (Invoke-PackageSmoke $missingRuntimeProbe $missingRuntimeRoot) `
        'missing Qt runtime'

    Write-Host 'The Windows package passed isolated and negative-control smoke tests.'
}
finally {
    Remove-ScratchRoot $scratchRoot $scratchBase
}
