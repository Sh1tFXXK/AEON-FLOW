param(
    [ValidateSet("all", "desktop", "relay", "android", "stop")]
    [string]$Mode = "all",

    [int]$Port = 8080,
    [int]$RelayPort = 8090,
    [int]$DiscoveryPort = 8091,
    [string]$Space = "home",
    [string]$RelayUrl = "",
    [string]$RelayDir = "",

    [switch]$NoRelay,
    [switch]$NoRestart,
    [switch]$NoFirewall,
    [switch]$NoNetworkProfile,
    [switch]$NoInstall,
    [switch]$UsbReverse,
    [switch]$Release
)

$ErrorActionPreference = "Stop"

$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
$RootPath = $Root.Path

function Add-PathIfExists {
    param([string]$Path)

    if ((Test-Path $Path) -and (($env:PATH -split ';') -notcontains $Path)) {
        $env:PATH = "$Path;$env:PATH"
    }
}

function Use-AeonBuildPath {
    Add-PathIfExists (Join-Path $env:USERPROFILE "scoop\apps\gcc\current\bin")
    Add-PathIfExists (Join-Path $env:USERPROFILE "scoop\shims")
}

function Test-IsAdministrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Ensure-AeonFirewallRule {
    param(
        [int]$Port,
        [string]$Label,
        [string]$Protocol = "TCP"
    )

    if ($NoFirewall) {
        return
    }
    if (-not (Test-IsAdministrator)) {
        Write-Warning "Cannot create firewall rule for $Label port $Port without Administrator rights. Run PowerShell as Administrator, or run: New-NetFirewallRule -DisplayName 'AEON Flow $Label $Protocol $Port' -Direction Inbound -Action Allow -Protocol $Protocol -LocalPort $Port -Profile Any"
        return
    }

    $name = "AEON Flow $Label $Protocol $Port"
    $existing = Get-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue
    if ($existing) {
        if ($existing.Enabled -ne "True") {
            Enable-NetFirewallRule -DisplayName $name | Out-Null
        }
        return
    }

    Write-Host "Opening Windows Firewall for $Label on $Protocol $Port"
    New-NetFirewallRule `
        -DisplayName $name `
        -Direction Inbound `
        -Action Allow `
        -Protocol $Protocol `
        -LocalPort $Port `
        -Profile Any | Out-Null
}

function Ensure-AeonFirewallRules {
    param(
        [int]$UiPort,
        [int]$RelayPort,
        [int]$DiscoveryPort,
        [bool]$IncludeUi,
        [bool]$IncludeRelay
    )

    if ($IncludeUi) {
        Ensure-AeonFirewallRule -Port $UiPort -Label "UI"
    }
    if ($IncludeRelay) {
        Ensure-AeonFirewallRule -Port $RelayPort -Label "Relay"
    }
    Ensure-AeonFirewallRule -Port $DiscoveryPort -Label "Discovery" -Protocol "UDP"
}

function Ensure-AeonNetworkProfile {
    if ($NoNetworkProfile) {
        return
    }
    if (-not (Test-IsAdministrator)) {
        Write-Warning "Cannot switch active LAN network profile to Private without Administrator rights."
        return
    }

    Get-NetConnectionProfile -ErrorAction SilentlyContinue |
        Where-Object {
            $_.IPv4Connectivity -ne "Disconnected" -and
            $_.InterfaceAlias -notmatch 'VMware|Virtual|vEthernet|Tailscale|Meta|Loopback|Docker|WSL'
        } |
        ForEach-Object {
            if ($_.NetworkCategory -ne "Private") {
                Write-Host "Setting network profile to Private for $($_.InterfaceAlias)"
                Set-NetConnectionProfile -InterfaceIndex $_.InterfaceIndex -NetworkCategory Private
            }
        }
}

function Set-AeonLanIpEnvironment {
    $ips = Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue |
        Where-Object {
            $_.AddressState -eq "Preferred" -and
            $_.IPAddress -notlike "127.*" -and
            $_.InterfaceAlias -notmatch 'VMware|Virtual|vEthernet|Tailscale|Meta|Loopback|Docker|WSL'
        } |
        Sort-Object @{Expression={ if ($_.InterfaceAlias -match 'WLAN|Wi-Fi|Wireless') { 0 } elseif ($_.InterfaceAlias -match 'Ethernet') { 1 } else { 2 } }}, InterfaceAlias |
        Select-Object -ExpandProperty IPAddress

    if ($ips) {
        $env:AEON_LAN_IPS = ($ips -join ",")
        Write-Host "AEON LAN IPs: $env:AEON_LAN_IPS"
    }
}

function Get-PortOwners {
    param([int[]]$Ports)

    Get-NetTCPConnection -LocalPort $Ports -State Listen -ErrorAction SilentlyContinue |
        Select-Object -ExpandProperty OwningProcess -Unique
}

function Get-ProcessInfo {
    param([int]$ProcessId)

    Get-CimInstance Win32_Process -Filter "ProcessId = $ProcessId" -ErrorAction SilentlyContinue
}

function Test-AeonSyncProcess {
    param($ProcessInfo)

    if ($null -eq $ProcessInfo) {
        return $false
    }
    $commandLine = [string]$ProcessInfo.CommandLine
    $exePath = [string]$ProcessInfo.ExecutablePath
    $rootLower = $RootPath.ToLowerInvariant()
    return (
        $ProcessInfo.Name -eq "aeon-sync.exe" -and
        (
            $commandLine.ToLowerInvariant().Contains($rootLower) -or
            $exePath.ToLowerInvariant().Contains($rootLower)
        )
    )
}

function Stop-AeonSyncProcesses {
    param([int[]]$Ports = @())

    $pids = @()
    if ($Ports.Count -gt 0) {
        $pids += Get-PortOwners $Ports
    }
    $pids += Get-CimInstance Win32_Process -Filter "Name = 'aeon-sync.exe'" -ErrorAction SilentlyContinue |
        Where-Object { Test-AeonSyncProcess $_ } |
        Select-Object -ExpandProperty ProcessId

    foreach ($processId in ($pids | Sort-Object -Unique)) {
        $info = Get-ProcessInfo $processId
        if (Test-AeonSyncProcess $info) {
            Write-Host "Stopping existing AEON sync process PID $processId"
            Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue
            try {
                Wait-Process -Id $processId -Timeout 5 -ErrorAction SilentlyContinue
            } catch {
            }
        }
    }
}

function Prepare-AeonPorts {
    param([int[]]$Ports)

    $owners = @(Get-PortOwners $Ports)
    if ($owners.Count -eq 0) {
        return
    }

    foreach ($processId in $owners) {
        $info = Get-ProcessInfo $processId
        if (Test-AeonSyncProcess $info) {
            if ($NoRestart) {
                throw "Port is already used by AEON sync PID $processId. Re-run without -NoRestart, or run .\scripts\aeon.ps1 -Mode stop first."
            }
            Write-Host "Port already has AEON sync PID $processId; restarting it."
            Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue
            try {
                Wait-Process -Id $processId -Timeout 5 -ErrorAction SilentlyContinue
            } catch {
            }
        } else {
            $name = if ($info) { $info.Name } else { "unknown" }
            throw "Port is already used by PID $processId ($name). Choose another port with -Port/-RelayPort, or stop that process."
        }
    }
}

function Invoke-CargoAeonSync {
    param([string[]]$SyncArgs)

    Use-AeonBuildPath
    Set-AeonLanIpEnvironment
    Push-Location (Join-Path $Root "aeon-sync")
    try {
        $cargoArgs = @("run")
        if ($Release) {
            $cargoArgs += "--release"
        }
        $cargoArgs += "--"
        $cargoArgs += $SyncArgs
        & cargo @cargoArgs
    } finally {
        Pop-Location
    }
}

function Invoke-AndroidBuild {
    Push-Location (Join-Path $Root "aeon-android")
    try {
        if (Test-Path ".\gradlew.bat") {
            & .\gradlew.bat assembleDebug
        } else {
            & .\gradlew assembleDebug
        }
    } finally {
        Pop-Location
    }
}

function Invoke-AndroidInstall {
    $adb = Get-Command adb -ErrorAction SilentlyContinue
    if (-not $adb) {
        Write-Host "adb not found; APK built but not installed."
        return
    }

    $devices = & adb devices |
        Select-String -Pattern "`tdevice$" |
        ForEach-Object { ($_.Line -split "`t")[0] }
    if (-not $devices) {
        Write-Host "No adb device found; APK built but not installed."
        return
    }

    if ($UsbReverse) {
        Write-Host "Configuring adb reverse for AEON UI and Relay"
        & adb reverse tcp:$Port tcp:$Port | Out-Null
        & adb reverse tcp:$RelayPort tcp:$RelayPort | Out-Null
    } else {
        Write-Host "Using AEON wireless discovery; clearing stale adb reverse for AEON ports"
        & adb reverse --remove tcp:$Port 2>$null | Out-Null
        & adb reverse --remove tcp:$RelayPort 2>$null | Out-Null
    }

    $apk = Join-Path $Root "aeon-android\app\build\outputs\apk\debug\app-debug.apk"
    Write-Host "Installing $apk"
    & adb install -r $apk
    Write-Host "Launching AEON Capture"
    & adb shell am start -n flow.aeon.capture/.MainActivity | Out-Null
}

switch ($Mode) {
    "all" {
        if ($NoRelay) {
            Prepare-AeonPorts @($Port)
        } else {
            Prepare-AeonPorts @($Port, $RelayPort)
        }
        Ensure-AeonNetworkProfile
        Ensure-AeonFirewallRules -UiPort $Port -RelayPort $RelayPort -DiscoveryPort $DiscoveryPort -IncludeUi $true -IncludeRelay (-not $NoRelay)
        $syncArgs = @("start", "--port", "$Port", "--relay-port", "$RelayPort", "--relay-space", $Space, "--discovery-port", "$DiscoveryPort")
        if ($NoRelay) {
            $syncArgs += "--no-relay"
        } else {
            $syncArgs += "--with-relay"
        }
        if ($RelayUrl.Trim()) {
            $syncArgs += @("--relay-url", $RelayUrl.Trim())
        }
        if ($RelayDir.Trim()) {
            $syncArgs += @("--relay-dir", $RelayDir.Trim())
        }
        Invoke-CargoAeonSync $syncArgs
    }
    "desktop" {
        Prepare-AeonPorts @($Port)
        Ensure-AeonNetworkProfile
        Ensure-AeonFirewallRules -UiPort $Port -RelayPort $RelayPort -DiscoveryPort $DiscoveryPort -IncludeUi $true -IncludeRelay $false
        $syncArgs = @("start", "--port", "$Port", "--no-relay", "--relay-space", $Space, "--discovery-port", "$DiscoveryPort")
        if ($RelayUrl.Trim()) {
            $syncArgs += @("--relay-url", $RelayUrl.Trim())
        }
        Invoke-CargoAeonSync $syncArgs
    }
    "relay" {
        Prepare-AeonPorts @($RelayPort)
        Ensure-AeonFirewallRules -UiPort $Port -RelayPort $RelayPort -DiscoveryPort $DiscoveryPort -IncludeUi $false -IncludeRelay $true
        $syncArgs = @("relay", "--port", "$RelayPort", "--space", $Space)
        if ($RelayDir.Trim()) {
            $syncArgs += @("--dir", $RelayDir.Trim())
        }
        Invoke-CargoAeonSync $syncArgs
    }
    "android" {
        Invoke-AndroidBuild
        if (-not $NoInstall) {
            Invoke-AndroidInstall
        }
    }
    "stop" {
        Stop-AeonSyncProcesses @($Port, $RelayPort)
    }
}
