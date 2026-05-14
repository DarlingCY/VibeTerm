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

# Create parent menu key
$backgroundParentKey = "HKCU:\Software\Classes\Directory\Background\shell\VibeTerm"
$directoryParentKey = "HKCU:\Software\Classes\Directory\shell\VibeTerm"

foreach ($parentKey in @($backgroundParentKey, $directoryParentKey)) {
    # Remove existing entries first
    if (Test-Path -LiteralPath $parentKey) {
        Remove-Item -LiteralPath $parentKey -Recurse -Force
    }
    
    # Create parent menu entry
    New-Item -Path $parentKey -Force | Out-Null
    New-ItemProperty -Path $parentKey -Name "MUIVerb" -Value "VibeTerm" -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $parentKey -Name "Icon" -Value $ExePath -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $parentKey -Name "Position" -Value "Top" -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $parentKey -Name "SubCommands" -Value "" -PropertyType String -Force | Out-Null
    
    # Create sub-menu entries
    # 1. Add to current tab (add-pane)
    $addPaneKey = Join-Path $parentKey "shell\add-pane"
    New-Item -Path $addPaneKey -Force | Out-Null
    New-ItemProperty -Path $addPaneKey -Name "MUIVerb" -Value "添加到当前 Tab（追加 Pane）" -PropertyType String -Force | Out-Null
    
    $addPaneCommandKey = Join-Path $addPaneKey "command"
    New-Item -Path $addPaneCommandKey -Force | Out-Null
    $addPaneCommand = '"{0}" --cwd "%V" --action add-pane' -f $ExePath
    Set-ItemProperty -Path $addPaneCommandKey -Name "(default)" -Value $addPaneCommand
    
    # 2. Open in new tab (new-tab)
    $newTabKey = Join-Path $parentKey "shell\new-tab"
    New-Item -Path $newTabKey -Force | Out-Null
    New-ItemProperty -Path $newTabKey -Name "MUIVerb" -Value "新开 Tab 和 Pane" -PropertyType String -Force | Out-Null
    
    $newTabCommandKey = Join-Path $newTabKey "command"
    New-Item -Path $newTabCommandKey -Force | Out-Null
    $newTabCommand = '"{0}" --cwd "%V" --action new-tab' -f $ExePath
    Set-ItemProperty -Path $newTabCommandKey -Name "(default)" -Value $newTabCommand
}

Write-Host "Installed Windows context menu for VibeTerm:"
Write-Host "  - 添加到当前 Tab（追加 Pane） (add-pane)"
Write-Host "  - 新开 Tab 和 Pane (new-tab)"
Write-Host "Executable: $ExePath"
