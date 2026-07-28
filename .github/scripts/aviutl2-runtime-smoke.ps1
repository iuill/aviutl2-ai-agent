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

function Approve-AviUtl2PluginTrustDialog {
    param(
        [Parameter(Mandatory = $true)]
        [int]$ProcessId
    )

    try {
        Add-Type -AssemblyName UIAutomationClient
        Add-Type -AssemblyName UIAutomationTypes
        $name = [System.Windows.Automation.PropertyCondition]::new(
            [System.Windows.Automation.AutomationElement]::NameProperty,
            "このプラグイン・スクリプトを信頼して使用する"
        )
        $owner = [System.Windows.Automation.PropertyCondition]::new(
            [System.Windows.Automation.AutomationElement]::ProcessIdProperty,
            $ProcessId
        )
        $condition = [System.Windows.Automation.AndCondition]::new($name, $owner)
        $button = [System.Windows.Automation.AutomationElement]::RootElement.FindFirst(
            [System.Windows.Automation.TreeScope]::Descendants,
            $condition
        )
        if ($null -eq $button) {
            return $false
        }

        $invoke = $button.GetCurrentPattern(
            [System.Windows.Automation.InvokePattern]::Pattern
        )
        $invoke.Invoke()
        return $true
    }
    catch {
        return $false
    }
}

try {
    Invoke-WebRequest -Uri $AviUtl2Url -OutFile $archive
    $actualHash = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLowerInvariant()
    [ordered]@{
        url = $AviUtl2Url
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

        if (Approve-AviUtl2PluginTrustDialog -ProcessId $process.Id) {
            $trustApprovals++
        }

        $healthOutput = & $cli health 2>&1 | Out-String
        $healthExitCode = $LASTEXITCODE
    } while ($healthExitCode -ne 0 -and [DateTime]::UtcNow -lt $deadline)

    $healthOutput | Set-Content -Encoding utf8 (Join-Path $output "health.json")
    if ($healthExitCode -ne 0) {
        throw "/healthz did not become ready within 45 seconds"
    }

    $readOutput = & $cli read-section 2>&1 | Out-String
    $readExitCode = $LASTEXITCODE
    $readOutput | Set-Content -Encoding utf8 (Join-Path $output "read-section.json")
    if ($readExitCode -ne 0) {
        throw "read-section failed with exit code $readExitCode"
    }

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
