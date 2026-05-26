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
        Label = "VibeTerm：添加到当前 Tab（追加 Pane）"
        Action = "add-pane"
    },
    @{
        KeyName = "VibeTermNewTab"
        Label = "VibeTerm：新开 Tab 和 Pane"
        Action = "new-tab"
    }
)

$parentRoots = @(
    @{ Path = "HKCU:\Software\Classes\Directory\Background\shell"; CwdArg = "%V" },
    @{ Path = "HKCU:\Software\Classes\Directory\shell"; CwdArg = "%1" },
    @{ Path = "HKCU:\Software\Classes\Folder\shell"; CwdArg = "%1" },
    @{ Path = "HKCU:\Software\Classes\Drive\shell"; CwdArg = "%1" },
    @{ Path = "HKCU:\Software\Classes\DesktopBackground\shell"; CwdArg = "%V" }
)

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
        Set-ItemProperty -Path $commandKey -Name "(default)" -Value $command
    }
}

Write-Host "Installed Windows context menu for VibeTerm:"
Write-Host "  - VibeTerm：添加到当前 Tab（追加 Pane）"
Write-Host "  - VibeTerm：新开 Tab 和 Pane"
Write-Host "Executable: $ExePath"
