param(
    [string]$ExePath = ""
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ExePath)) {
    $ExePath = Join-Path (Resolve-Path (Join-Path $PSScriptRoot "..\target\release")).Path "vibeterm.exe"
}

if (-not (Test-Path -LiteralPath $ExePath)) {
    throw "vibeterm.exe not found: $ExePath. Run `cargo build --release` first, or pass -ExePath."
}

$ExePath = (Resolve-Path -LiteralPath $ExePath).Path

$menuSpecs = @(
    @{
        KeyName = "VibeTermAddPane"
        Label = "VibeTerm: Add Pane Here"
        Action = "add-pane"
    },
    @{
        KeyName = "VibeTermNewTab"
        Label = "VibeTerm: New Tab Here"
        Action = "new-tab"
    }
)

$parentRoots = @(
    @{ Path = "HKCU:\Software\Classes\Directory\Background\shell"; CwdArg = "%V" },
    @{ Path = "HKCU:\Software\Classes\Directory\shell"; CwdArg = "%1" },
    @{ Path = "HKCU:\Software\Classes\Drive\shell"; CwdArg = "%1" },
    @{ Path = "HKCU:\Software\Classes\DesktopBackground\shell"; CwdArg = "%V" }
)

$legacyKeys = @(
    "HKCU:\Software\Classes\Folder\shell\VibeTermAddPane",
    "HKCU:\Software\Classes\Folder\shell\VibeTermNewTab"
)

foreach ($key in $legacyKeys) {
    if (Test-Path -LiteralPath $key) {
        Remove-Item -LiteralPath $key -Recurse -Force
    }
}

$doubleClickShellRoots = @(
    "HKCU:\Software\Classes\Directory\shell",
    "HKCU:\Software\Classes\Folder\shell",
    "HKCU:\Software\Classes\Drive\shell"
)

foreach ($root in $doubleClickShellRoots) {
    New-Item -Path $root -Force | Out-Null
    Set-Item -LiteralPath $root -Value "none"
}

foreach ($rootSpec in $parentRoots) {
    $root = $rootSpec.Path
    $cwdArg = $rootSpec.CwdArg
    foreach ($spec in $menuSpecs) {
        $key = Join-Path $root $spec.KeyName

        if (Test-Path -LiteralPath $key) {
            Remove-Item -LiteralPath $key -Recurse -Force
        }

        New-Item -Path $key -Force | Out-Null
        New-ItemProperty -Path $key -Name "MUIVerb" -Value $spec.Label -PropertyType String -Force | Out-Null
        New-ItemProperty -Path $key -Name "Icon" -Value $ExePath -PropertyType String -Force | Out-Null
        New-ItemProperty -Path $key -Name "Position" -Value "Top" -PropertyType String -Force | Out-Null

        $commandKey = Join-Path $key "command"
        New-Item -Path $commandKey -Force | Out-Null
        $command = '"{0}" --cwd "{1}" --action {2}' -f $ExePath, $cwdArg, $spec.Action
        Set-Item -LiteralPath $commandKey -Value $command
    }
}

if (-not ([System.Management.Automation.PSTypeName]'VibeTerm.NativeMethods').Type) {
    Add-Type @"
using System;
using System.Runtime.InteropServices;

namespace VibeTerm {
    public static class NativeMethods {
        [DllImport("shell32.dll")]
        public static extern void SHChangeNotify(int wEventId, uint uFlags, IntPtr dwItem1, IntPtr dwItem2);
    }
}
"@
}
[VibeTerm.NativeMethods]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)

Write-Host "Installed Windows context menu for VibeTerm:"
Write-Host "  - VibeTerm: Add Pane Here"
Write-Host "  - VibeTerm: New Tab Here"
Write-Host "Executable: $ExePath"
