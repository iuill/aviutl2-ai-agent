[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, ParameterSetName = "Download")]
    [string]$AviUtl2Url,

    [Parameter(Mandatory = $true, ParameterSetName = "Download")]
    [string]$AviUtl2Sha256,

    [Parameter(Mandatory = $true, ParameterSetName = "Installed")]
    [string]$AviUtl2Directory,

    [Parameter(Mandatory = $true)]
    [string]$PluginDll,

    [Parameter(Mandatory = $true)]
    [string]$CliExe,

    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$WM_CLOSE = 0x0010

$output = [System.IO.Path]::GetFullPath($OutputDirectory)
$tempRoot = if ([string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
    [System.IO.Path]::GetTempPath()
}
else {
    $env:RUNNER_TEMP
}
$work = Join-Path $tempRoot "aviutl2-runtime-spike"
$archive = Join-Path $work "aviutl2.zip"
$app = Join-Path $work "app"
$pluginDirectory = Join-Path $app "data\Plugin"
$aviutl2 = Join-Path $app "aviutl2.exe"
$cli = [System.IO.Path]::GetFullPath($CliExe)
$plugin = [System.IO.Path]::GetFullPath($PluginDll)

New-Item -ItemType Directory -Force -Path $output, $work | Out-Null
if (Test-Path -LiteralPath $app) {
    Remove-Item -LiteralPath $app -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $app, $pluginDirectory | Out-Null

$environment = [ordered]@{
    observedAtUtc = [DateTime]::UtcNow.ToString("o")
    runnerImage = $env:ImageOS
    runnerImageVersion = $env:ImageVersion
    os = Get-CimInstance Win32_OperatingSystem |
        Select-Object Caption, Version, BuildNumber
    cpu = Get-CimInstance Win32_Processor |
        Select-Object Name, Manufacturer, NumberOfCores, NumberOfLogicalProcessors
    videoControllers = @(
        Get-CimInstance Win32_VideoController |
            Select-Object Name, DriverVersion, VideoProcessor, AdapterRAM
    )
}
$environment | ConvertTo-Json -Depth 5 |
    Set-Content -Encoding utf8 (Join-Path $output "environment.json")

$process = $null
$conflictProcess = $null
$portBlocker = $null
$succeeded = $false
$failure = $null
$trustApprovals = 0
$conflictTrustApprovals = 0
$observationLogs = [ordered]@{
    AVIUTL2_AI_AGENT_SCENE_OBSERVATION_LOG = "scene-observations.jsonl"
    AVIUTL2_AI_AGENT_EVENT_OBSERVATION_LOG = "event-observations.jsonl"
    AVIUTL2_AI_AGENT_OBJECT_OBSERVATION_LOG = "object-observations.jsonl"
}
$previousObservationLogs = @{}
foreach ($environmentName in $observationLogs.Keys) {
    $previousObservationLogs[$environmentName] = [Environment]::GetEnvironmentVariable(
        $environmentName,
        "Process"
    )
    [Environment]::SetEnvironmentVariable(
        $environmentName,
        (Join-Path $output $observationLogs[$environmentName]),
        "Process"
    )
}
@(
    "scene-observations.jsonl",
    "event-observations.jsonl",
    "object-observations.jsonl",
    "plugin-lifecycle.jsonl",
    "port-conflict-plugin-lifecycle.jsonl"
) | ForEach-Object {
    $staleLog = Join-Path $output $_
    if (Test-Path -LiteralPath $staleLog) {
        Remove-Item -LiteralPath $staleLog -Force
    }
}

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class AviUtl2NativeMethods
{
    private delegate bool EnumWindowsProc(IntPtr window, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(
        EnumWindowsProc callback,
        IntPtr lParam
    );

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetClassName(
        IntPtr window,
        StringBuilder className,
        int maxCount
    );

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowText(
        IntPtr window,
        StringBuilder windowText,
        int maxCount
    );

    [DllImport("user32.dll")]
    public static extern IntPtr GetDlgItem(IntPtr dialog, int itemId);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(
        IntPtr window,
        out uint processId
    );

    public static IntPtr FindWindowForProcessClassAndTitle(
        uint processId,
        string expectedClass,
        string expectedTitle
    )
    {
        IntPtr result = IntPtr.Zero;
        EnumWindows(
            delegate(IntPtr window, IntPtr lParam)
            {
                uint windowProcessId;
                GetWindowThreadProcessId(window, out windowProcessId);
                if (windowProcessId != processId)
                {
                    return true;
                }

                StringBuilder className = new StringBuilder(256);
                GetClassName(window, className, className.Capacity);
                if (className.ToString() != expectedClass)
                {
                    return true;
                }

                StringBuilder windowText = new StringBuilder(256);
                GetWindowText(window, windowText, windowText.Capacity);
                if (windowText.ToString() != expectedTitle)
                {
                    return true;
                }

                result = window;
                return false;
            },
            IntPtr.Zero
        );
        return result;
    }

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool PostMessage(
        IntPtr window,
        uint message,
        IntPtr wParam,
        IntPtr lParam
    );

}
"@

function Invoke-CliCapture {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$Arguments
    )

    $previousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $capturedOutput = & $cli @Arguments 2>&1 | Out-String
        $capturedExitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }

    return [pscustomobject]@{
        Output = $capturedOutput
        ExitCode = $capturedExitCode
    }
}

function Approve-AviUtl2PluginTrustDialog {
    param(
        [Parameter(Mandatory = $true)]
        [int]$ProcessId
    )

    # Build the observed Japanese title from code points because Windows
    # PowerShell 5.1 reads BOM-less UTF-8 scripts using the legacy code page.
    $trustDialogTitle = -join [char[]]@(
        0x30B9, 0x30AF, 0x30EA, 0x30D7, 0x30C8, 0x30FB, 0x30D7,
        0x30E9, 0x30B0, 0x30A4, 0x30F3, 0x306E, 0x8FFD, 0x52A0
    )
    $dialog = [AviUtl2NativeMethods]::FindWindowForProcessClassAndTitle(
        [uint32]$ProcessId,
        "#32770",
        $trustDialogTitle
    )
    if ($dialog -eq [IntPtr]::Zero) {
        return $false
    }

    # FlaUI/UIA3 observation identified control ID 6 as the trust button.
    $button = [AviUtl2NativeMethods]::GetDlgItem($dialog, 6)
    if ($button -eq [IntPtr]::Zero) {
        return $false
    }

    return [AviUtl2NativeMethods]::PostMessage(
        $button,
        0x00F5,
        [IntPtr]::Zero,
        [IntPtr]::Zero
    )
}

try {
    if ($PSCmdlet.ParameterSetName -eq "Download") {
        $downloadSource = "cache"
        if (-not (Test-Path -LiteralPath $archive)) {
            Invoke-WebRequest -Uri $AviUtl2Url -OutFile $archive
            $downloadSource = "official-site"
        }

        $actualHash = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLowerInvariant()
        [ordered]@{
            url = $AviUtl2Url
            source = $downloadSource
            expectedSha256 = $AviUtl2Sha256.ToLowerInvariant()
            actualSha256 = $actualHash
        } | ConvertTo-Json |
            Set-Content -Encoding utf8 (Join-Path $output "aviutl2-source.json")

        if ($actualHash -ne $AviUtl2Sha256.ToLowerInvariant()) {
            throw "AviUtl2 archive SHA-256 mismatch"
        }

        Expand-Archive -LiteralPath $archive -DestinationPath $app
    }
    else {
        $installedApp = [System.IO.Path]::GetFullPath($AviUtl2Directory)
        $installedExe = Join-Path $installedApp "aviutl2.exe"
        if (-not (Test-Path -LiteralPath $installedExe -PathType Leaf)) {
            throw "aviutl2.exe was not found in AviUtl2Directory"
        }
        Copy-Item -Path (Join-Path $installedApp "*") `
            -Destination $app `
            -Recurse `
            -Force
        [ordered]@{
            source = "preinstalled-copy"
            executableSha256 = (
                Get-FileHash -Algorithm SHA256 $installedExe
            ).Hash.ToLowerInvariant()
        } | ConvertTo-Json |
            Set-Content -Encoding utf8 (Join-Path $output "aviutl2-source.json")
    }
    Copy-Item -LiteralPath $plugin `
        -Destination (Join-Path $pluginDirectory "aviutl2-agent-plugin.aux2")
    Copy-Item -LiteralPath $cli -Destination (Join-Path $app "aviutl2-agent.exe")

    $lifecycleLog = Join-Path $output "plugin-lifecycle.jsonl"
    $env:AVIUTL2_AI_AGENT_PHASE1_LIFECYCLE_LOG = $lifecycleLog
    $process = Start-Process -FilePath $aviutl2 -WorkingDirectory $app -PassThru
    [ordered]@{
        processId = $process.Id
        startedAtUtc = [DateTime]::UtcNow.ToString("o")
    } | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $output "process-start.json")

    $deadline = [DateTime]::UtcNow.AddSeconds(45)
    $healthOutput = $null
    do {
        Start-Sleep -Milliseconds 500
        $process.Refresh()
        if ($process.HasExited) {
            throw "AviUtl2 exited before /healthz became ready (exit code $($process.ExitCode))"
        }

        if (
            $trustApprovals -eq 0 -and
            (Approve-AviUtl2PluginTrustDialog -ProcessId $process.Id)
        ) {
            $trustApprovals++
        }

        $healthResult = Invoke-CliCapture -Arguments @("health")
        $healthOutput = $healthResult.Output
        $healthExitCode = $healthResult.ExitCode
    } while ($healthExitCode -ne 0 -and [DateTime]::UtcNow -lt $deadline)

    $healthOutput | Set-Content -Encoding utf8 (Join-Path $output "health.json")
    if ($healthExitCode -ne 0) {
        throw "/healthz did not become ready within 45 seconds"
    }

    $statusDeadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        $statusResult = Invoke-CliCapture -Arguments @("status")
        $statusOutput = $statusResult.Output
        $statusExitCode = $statusResult.ExitCode
        if ($statusExitCode -ne 0 -and [DateTime]::UtcNow -lt $statusDeadline) {
            Start-Sleep -Milliseconds 250
        }
    } while ($statusExitCode -ne 0 -and [DateTime]::UtcNow -lt $statusDeadline)

    $statusOutput | Set-Content -Encoding utf8 (Join-Path $output "status.json")
    if ($statusExitCode -ne 0) {
        throw "status did not become ready within 15 seconds"
    }

    $sceneDeadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        $sceneResult = Invoke-CliCapture -Arguments @("current-scene")
        $sceneOutput = $sceneResult.Output
        $sceneExitCode = $sceneResult.ExitCode
        if ($sceneExitCode -ne 0 -and [DateTime]::UtcNow -lt $sceneDeadline) {
            Start-Sleep -Milliseconds 250
        }
    } while ($sceneExitCode -ne 0 -and [DateTime]::UtcNow -lt $sceneDeadline)
    $sceneOutput | Set-Content -Encoding utf8 (Join-Path $output "current-scene.json")
    if ($sceneExitCode -ne 0) {
        throw "current-scene failed with exit code $sceneExitCode"
    }

    $timelineDeadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        $timelineResult = Invoke-CliCapture -Arguments @("current-timeline")
        if ($timelineResult.ExitCode -ne 0 -and [DateTime]::UtcNow -lt $timelineDeadline) {
            Start-Sleep -Milliseconds 250
        }
    } while ($timelineResult.ExitCode -ne 0 -and [DateTime]::UtcNow -lt $timelineDeadline)
    $timelineResult.Output |
        Set-Content -Encoding utf8 (Join-Path $output "current-timeline.json")
    if ($timelineResult.ExitCode -ne 0) {
        throw "current-timeline failed with exit code $($timelineResult.ExitCode)"
    }

    $objectsResult = Invoke-CliCapture -Arguments @("current-objects")
    $objectsResult.Output |
        Set-Content -Encoding utf8 (Join-Path $output "current-objects.json")
    if ($objectsResult.ExitCode -ne 0) {
        throw "current-objects failed with exit code $($objectsResult.ExitCode)"
    }

    $idleClient = [System.Net.Sockets.TcpClient]::new()
    $idleClient.Connect("127.0.0.1", 7890)
    $shutdownStarted = [DateTime]::UtcNow
    try {
        $process.Refresh()
        $window = $process.MainWindowHandle
        if ($window -eq [IntPtr]::Zero) {
            throw "AviUtl2 main window was not available for graceful shutdown"
        }
        if (-not [AviUtl2NativeMethods]::PostMessage(
            $window,
            $WM_CLOSE,
            [IntPtr]::Zero,
            [IntPtr]::Zero
        )) {
            throw "Failed to post WM_CLOSE to AviUtl2"
        }
        if (-not $process.WaitForExit(30000)) {
            throw "AviUtl2 did not exit within 30 seconds after WM_CLOSE"
        }
    }
    finally {
        $idleClient.Dispose()
    }

    if (-not (Test-Path -LiteralPath $lifecycleLog)) {
        throw "Plugin lifecycle log was not created"
    }
    $lifecycleRecords = @(
        Get-Content -LiteralPath $lifecycleLog |
            ForEach-Object { $_ | ConvertFrom-Json }
    )
    $expectedEvents = @(
        "plugin_drop_started",
        "http_workers_joined",
        "plugin_drop_completed"
    )
    $actualEvents = @($lifecycleRecords | ForEach-Object { $_.event })
    # Verify both the complete event set and its order so lifecycle changes
    # require an intentional update to this shutdown contract.
    if (($actualEvents -join "`n") -ne ($expectedEvents -join "`n")) {
        throw "Unexpected plugin lifecycle sequence: $($actualEvents -join ', ')"
    }
    $joined = $lifecycleRecords[1]
    if ($joined.workerCount -ne 4 -or $joined.joinPanics -ne 0) {
        throw "Unexpected worker join result: $($joined | ConvertTo-Json -Compress)"
    }
    if ($process.ExitCode -ne 0) {
        throw "AviUtl2 exited with code $($process.ExitCode)"
    }

    $portProbe = [System.Net.Sockets.TcpListener]::new(
        [System.Net.IPAddress]::Loopback,
        7890
    )
    try {
        $portProbe.Start()
    }
    finally {
        $portProbe.Stop()
    }

    [ordered]@{
        requestedAtUtc = $shutdownStarted.ToString("o")
        exitedAtUtc = [DateTime]::UtcNow.ToString("o")
        elapsedMilliseconds = [int](
            ([DateTime]::UtcNow - $shutdownStarted).TotalMilliseconds
        )
        exitCode = $process.ExitCode
        idleClientConnected = $true
        lifecycleEvents = $actualEvents
        joinedWorkers = $joined.workerCount
        joinPanics = $joined.joinPanics
        portRebindSucceeded = $true
    } | ConvertTo-Json -Depth 4 |
        Set-Content -Encoding utf8 (Join-Path $output "graceful-shutdown.json")

    $conflictLifecycleLog = Join-Path $output "port-conflict-plugin-lifecycle.jsonl"
    $env:AVIUTL2_AI_AGENT_PHASE1_LIFECYCLE_LOG = $conflictLifecycleLog
    $portBlocker = [System.Net.Sockets.TcpListener]::new(
        [System.Net.IPAddress]::Loopback,
        7890
    )
    $portBlocker.Start()
    try {
        $conflictStarted = [DateTime]::UtcNow
        $conflictProcess = Start-Process `
            -FilePath $aviutl2 `
            -WorkingDirectory $app `
            -PassThru
        $conflictDeadline = [DateTime]::UtcNow.AddSeconds(30)
        do {
            Start-Sleep -Milliseconds 250
            $conflictProcess.Refresh()
            if ($conflictProcess.HasExited) {
                throw "AviUtl2 exited while port 7890 was occupied (exit code $($conflictProcess.ExitCode))"
            }
            if (
                $conflictTrustApprovals -eq 0 -and
                (Approve-AviUtl2PluginTrustDialog -ProcessId $conflictProcess.Id)
            ) {
                $conflictTrustApprovals++
            }
        } while (
            $conflictProcess.MainWindowHandle -eq [IntPtr]::Zero -and
            [DateTime]::UtcNow -lt $conflictDeadline
        )
        if ($conflictProcess.MainWindowHandle -eq [IntPtr]::Zero) {
            throw "AviUtl2 main window did not become available while port 7890 was occupied"
        }

        $conflictObservationDeadline = [DateTime]::UtcNow.AddSeconds(2)
        do {
            Start-Sleep -Milliseconds 250
            $conflictProcess.Refresh()
            if ($conflictProcess.HasExited) {
                throw "AviUtl2 did not remain running while port 7890 was occupied"
            }
            if (
                $conflictTrustApprovals -eq 0 -and
                (Approve-AviUtl2PluginTrustDialog -ProcessId $conflictProcess.Id)
            ) {
                $conflictTrustApprovals++
            }
        } while ([DateTime]::UtcNow -lt $conflictObservationDeadline)
        $conflictWindow = $conflictProcess.MainWindowHandle
        $conflictWindowTitle = $conflictProcess.MainWindowTitle
        $conflictResponding = $conflictProcess.Responding

        if (-not [AviUtl2NativeMethods]::PostMessage(
            $conflictWindow,
            $WM_CLOSE,
            [IntPtr]::Zero,
            [IntPtr]::Zero
        )) {
            throw "Failed to post WM_CLOSE during the port conflict observation"
        }
        if (-not $conflictProcess.WaitForExit(30000)) {
            throw "AviUtl2 did not exit after the port conflict observation"
        }
        if ($conflictProcess.ExitCode -ne 0) {
            throw "AviUtl2 exited with code $($conflictProcess.ExitCode) after the port conflict observation"
        }
        if (-not (Test-Path -LiteralPath $conflictLifecycleLog)) {
            throw "Plugin did not record the port conflict lifecycle"
        }
        $conflictLifecycleRecords = @(
            Get-Content -LiteralPath $conflictLifecycleLog |
                ForEach-Object { $_ | ConvertFrom-Json }
        )
        $expectedConflictEvents = @(
            "api_start_failed",
            "plugin_drop_started",
            "http_workers_joined",
            "plugin_drop_completed"
        )
        $actualConflictEvents = @(
            $conflictLifecycleRecords | ForEach-Object { $_.event }
        )
        # As above, require exact membership and order for the disabled API
        # lifecycle rather than accepting an arbitrary subsequence.
        if (
            ($actualConflictEvents -join "`n") -ne
            ($expectedConflictEvents -join "`n")
        ) {
            throw "Unexpected port conflict lifecycle sequence: $($actualConflictEvents -join ', ')"
        }
        $conflictJoinedRecords = @(
            $conflictLifecycleRecords |
                Where-Object { $_.event -eq "http_workers_joined" }
        )
        if ($conflictJoinedRecords.Count -ne 1) {
            throw "Expected one port conflict worker join record"
        }
        $conflictJoined = $conflictJoinedRecords[0]
        if ($conflictJoined.workerCount -ne 0 -or $conflictJoined.joinPanics -ne 0) {
            throw "Unexpected port conflict worker result: $($conflictJoined | ConvertTo-Json -Compress)"
        }

        [ordered]@{
            startedAtUtc = $conflictStarted.ToString("o")
            processId = $conflictProcess.Id
            mainWindowAvailable = $true
            mainWindowTitle = $conflictWindowTitle
            responding = $conflictResponding
            remainedRunningForObservation = $true
            lifecycleEvents = $actualConflictEvents
            joinedWorkers = $conflictJoined.workerCount
            joinPanics = $conflictJoined.joinPanics
            exitCode = $conflictProcess.ExitCode
        } | ConvertTo-Json |
            Set-Content -Encoding utf8 (Join-Path $output "port-conflict.json")
    }
    finally {
        $portBlocker.Stop()
        $portBlocker = $null
    }

    $succeeded = $true
}
catch {
    $failure = $_ | Out-String
    $failure | Set-Content -Encoding utf8 (Join-Path $output "failure.txt")
}
finally {
    if ($null -ne $portBlocker) {
        $portBlocker.Stop()
    }
    if ($null -ne $conflictProcess) {
        $conflictProcess.Refresh()
        if (-not $conflictProcess.HasExited) {
            Stop-Process -Id $conflictProcess.Id -Force
            $conflictProcess.WaitForExit(10000)
        }
    }

    [ordered]@{
        approvedPluginTrustDialogs = $trustApprovals
        approvedPortConflictTrustDialogs = $conflictTrustApprovals
    } | ConvertTo-Json |
        Set-Content -Encoding utf8 (Join-Path $output "trust-dialogs.json")

    $processState = [ordered]@{
        existed = $null -ne $process
        hadExitedBeforeCleanup = $null
        exitCodeBeforeCleanup = $null
        respondingBeforeCleanup = $null
        mainWindowTitleBeforeCleanup = $null
        mainWindowHandleBeforeCleanup = $null
    }
    if ($null -ne $process) {
        $process.Refresh()
        $processState.hadExitedBeforeCleanup = $process.HasExited
        if ($process.HasExited) {
            $processState.exitCodeBeforeCleanup = $process.ExitCode
        }
        else {
            $processState.respondingBeforeCleanup = $process.Responding
            $processState.mainWindowTitleBeforeCleanup = $process.MainWindowTitle
            $processState.mainWindowHandleBeforeCleanup = $process.MainWindowHandle.ToInt64()

            try {
                Add-Type -AssemblyName System.Drawing
                Add-Type -AssemblyName System.Windows.Forms
                $screen = [System.Windows.Forms.SystemInformation]::VirtualScreen
                $bitmap = [System.Drawing.Bitmap]::new($screen.Width, $screen.Height)
                $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
                try {
                    $graphics.CopyFromScreen(
                        $screen.Left,
                        $screen.Top,
                        0,
                        0,
                        $bitmap.Size
                    )
                    $bitmap.Save(
                        (Join-Path $output "desktop.png"),
                        [System.Drawing.Imaging.ImageFormat]::Png
                    )
                }
                finally {
                    $graphics.Dispose()
                    $bitmap.Dispose()
                }
            }
            catch {
                $_ | Out-String |
                    Set-Content -Encoding utf8 (Join-Path $output "screenshot-error.txt")
            }

            Stop-Process -Id $process.Id -Force
            $process.WaitForExit(10000)
        }
    }
    $processState | ConvertTo-Json |
        Set-Content -Encoding utf8 (Join-Path $output "process-cleanup.json")

    Get-Process |
        Sort-Object ProcessName |
        Select-Object ProcessName, Id, SessionId |
        ConvertTo-Json |
        Set-Content -Encoding utf8 (Join-Path $output "processes.json")

    Get-NetTCPConnection -LocalPort 7890 -ErrorAction SilentlyContinue |
        Select-Object State, LocalAddress, LocalPort, OwningProcess |
        ConvertTo-Json |
        Set-Content -Encoding utf8 (Join-Path $output "port-7890.json")

    foreach ($environmentName in $observationLogs.Keys) {
        [Environment]::SetEnvironmentVariable(
            $environmentName,
            $previousObservationLogs[$environmentName],
            "Process"
        )
    }
}

if (-not $succeeded) {
    Write-Error "AviUtl2 runtime spike failed: $failure"
    exit 1
}

Write-Host "AviUtl2 runtime spike succeeded."
