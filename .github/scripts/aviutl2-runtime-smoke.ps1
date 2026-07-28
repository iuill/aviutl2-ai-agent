[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$AviUtl2Url,

    [Parameter(Mandatory = $true)]
    [string]$AviUtl2Sha256,

    [Parameter(Mandatory = $true)]
    [string]$PluginDll,

    [Parameter(Mandatory = $true)]
    [string]$CliExe,

    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$output = [System.IO.Path]::GetFullPath($OutputDirectory)
$work = Join-Path $env:RUNNER_TEMP "aviutl2-runtime-spike"
$archive = Join-Path $work "aviutl2.zip"
$app = Join-Path $work "app"
$pluginDirectory = Join-Path $app "data\Plugin"
$aviutl2 = Join-Path $app "aviutl2.exe"
$cli = [System.IO.Path]::GetFullPath($CliExe)
$plugin = [System.IO.Path]::GetFullPath($PluginDll)

New-Item -ItemType Directory -Force -Path $output, $work, $app, $pluginDirectory | Out-Null

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
$succeeded = $false
$failure = $null
$trustApprovals = 0

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class AviUtl2NativeMethods
{
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr FindWindow(string className, string windowName);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr FindWindowEx(
        IntPtr parent,
        IntPtr childAfter,
        string className,
        string windowName
    );

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(
        IntPtr window,
        out uint processId
    );

    [DllImport("user32.dll")]
    public static extern IntPtr SendMessage(
        IntPtr window,
        uint message,
        IntPtr wParam,
        IntPtr lParam
    );

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool PostMessage(
        IntPtr window,
        uint message,
        IntPtr wParam,
        IntPtr lParam
    );

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr window, out Rect rect);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll")]
    public static extern void mouse_event(
        uint flags,
        uint x,
        uint y,
        uint data,
        UIntPtr extraInfo
    );
}
"@

function Approve-AviUtl2PluginTrustDialog {
    param(
        [Parameter(Mandatory = $true)]
        [int]$ProcessId
    )

    $aviutl2Process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($null -eq $aviutl2Process) {
        return $false
    }

    $aviutl2Process.Refresh()
    $window = $aviutl2Process.MainWindowHandle
    if ($window -eq [IntPtr]::Zero) {
        return $false
    }

    try {
        $rect = [AviUtl2NativeMethods+Rect]::new()
        if (-not [AviUtl2NativeMethods]::GetWindowRect($window, [ref]$rect)) {
            return $false
        }

        $x = [int](($rect.Left + $rect.Right) / 2) - 85
        $y = [int](($rect.Top + $rect.Bottom) / 2) + 88
        [void][AviUtl2NativeMethods]::SetForegroundWindow($window)
        [void][AviUtl2NativeMethods]::SetCursorPos($x, $y)
        [AviUtl2NativeMethods]::mouse_event(
            0x0002,
            0,
            0,
            0,
            [UIntPtr]::Zero
        )
        [AviUtl2NativeMethods]::mouse_event(
            0x0004,
            0,
            0,
            0,
            [UIntPtr]::Zero
        )
        return $true
    }
    catch {
        return $false
    }
}

try {
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
    } | ConvertTo-Json | Set-Content -Encoding utf8 (Join-Path $output "download.json")

    if ($actualHash -ne $AviUtl2Sha256.ToLowerInvariant()) {
        throw "AviUtl2 archive SHA-256 mismatch"
    }

    Expand-Archive -LiteralPath $archive -DestinationPath $app
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

        $healthOutput = & $cli health 2>&1 | Out-String
        $healthExitCode = $LASTEXITCODE
    } while ($healthExitCode -ne 0 -and [DateTime]::UtcNow -lt $deadline)

    $healthOutput | Set-Content -Encoding utf8 (Join-Path $output "health.json")
    if ($healthExitCode -ne 0) {
        throw "/healthz did not become ready within 45 seconds"
    }

    $statusOutput = & $cli status 2>&1 | Out-String
    $statusExitCode = $LASTEXITCODE
    $statusOutput | Set-Content -Encoding utf8 (Join-Path $output "status.json")
    if ($statusExitCode -ne 0) {
        throw "status failed with exit code $statusExitCode"
    }

    $sceneOutput = & $cli current-scene 2>&1 | Out-String
    $sceneExitCode = $LASTEXITCODE
    $sceneOutput | Set-Content -Encoding utf8 (Join-Path $output "current-scene.json")
    if ($sceneExitCode -ne 0) {
        throw "current-scene failed with exit code $sceneExitCode"
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
            0x0010,
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

    $succeeded = $true
}
catch {
    $failure = $_ | Out-String
    $failure | Set-Content -Encoding utf8 (Join-Path $output "failure.txt")
}
finally {
    [ordered]@{
        approvedPluginTrustDialogs = $trustApprovals
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
}

if (-not $succeeded) {
    Write-Error "AviUtl2 runtime spike failed: $failure"
    exit 1
}

Write-Host "AviUtl2 runtime spike succeeded."
