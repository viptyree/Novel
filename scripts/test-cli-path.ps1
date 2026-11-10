# Run only in an isolated Windows CI user. Restore registry values even on failure.
$ErrorActionPreference = 'Stop'
$script = Join-Path $PSScriptRoot '..\app\src-tauri\installer\cli-path.ps1'
$envKey = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
$marker = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\SodarieNovel\CLI')
$hadPath = $envKey.GetValueNames() -contains 'Path'
$oldPath = $envKey.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
$oldKind = if ($hadPath) { $envKey.GetValueKind('Path') } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
$oldOwned = $marker.GetValue('OwnedPath', $null)
function AssertEqual($actual, $expected) { if ($actual -ne $expected) { throw "Registry PATH assertion failed" } }
function UserPath { $envKey.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) }
try {
    $target = 'C:\Novel CLI Test'
    $base = '%USERPROFILE%\bin;' + (('C:\LongUnrelatedEntry;' * 150).TrimEnd(';'))
    $envKey.SetValue('Path', $base, [Microsoft.Win32.RegistryValueKind]::ExpandString)
    $marker.DeleteValue('OwnedPath', $false)
    & $script -Action Add -Directory $target
    AssertEqual (UserPath) ($base + ';' + $target)
    & $script -Action Add -Directory $target
    AssertEqual (UserPath) ($base + ';' + $target)
    # Preserve modifications made by the user after installation.
    $envKey.SetValue('Path', (UserPath) + ';C:\AddedLater', [Microsoft.Win32.RegistryValueKind]::ExpandString)
    & $script -Action Remove -Directory $target
    AssertEqual (UserPath) ($base + ';C:\AddedLater')
    # Never claim ownership of an entry that existed before installation.
    $envKey.SetValue('Path', $base + ';' + $target, [Microsoft.Win32.RegistryValueKind]::ExpandString)
    & $script -Action Add -Directory $target
    & $script -Action Remove -Directory $target
    AssertEqual (UserPath) ($base + ';' + $target)
    Write-Output 'PATH add/upgrade/remove/pre-existing/long-value tests passed.'
} finally {
    if ($hadPath) { $envKey.SetValue('Path', $oldPath, $oldKind) } else { $envKey.DeleteValue('Path', $false) }
    if ($null -ne $oldOwned) { $marker.SetValue('OwnedPath', $oldOwned) } else { $marker.DeleteValue('OwnedPath', $false) }
    $marker.Close()
    $envKey.Close()
}
