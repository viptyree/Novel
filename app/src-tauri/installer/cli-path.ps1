# Change only the current user's PATH. Read the registry directly to preserve %VARIABLES%
# and avoid NSIS string-length limits. Never delete projects or application data.
param(
    [Parameter(Mandatory=$true)][ValidateSet('Add','Remove')][string]$Action,
    [Parameter(Mandatory=$true)][string]$Directory
)
$ErrorActionPreference = 'Stop'
$marker = 'Software\SodarieNovel\CLI'
$environment = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
$ownership = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($marker)
try {
    $raw = [string]$environment.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $kind = [Microsoft.Win32.RegistryValueKind]::ExpandString
    if ($environment.GetValueNames() -contains 'Path') { $kind = $environment.GetValueKind('Path') }
    $owned = [string]$ownership.GetValue('OwnedPath', '')
    $normalize = { param([string]$entry) $entry.Trim().Trim('"').TrimEnd('\','/').ToLowerInvariant() }
    $target = & $normalize $Directory
    $entries = @($raw.Split(';'))
    $present = @($entries | Where-Object { (& $normalize $_) -eq $target }).Count -gt 0
    if ($Action -eq 'Add') {
        if (-not $present) {
            $next = if ($raw.Length -eq 0) { $Directory } else { $raw.TrimEnd(';') + ';' + $Directory }
            $environment.SetValue('Path', $next, $kind)
            $ownership.SetValue('OwnedPath', $Directory)
        }
        # A pre-existing entry is not owned by this installer. Preserve an existing
        # ownership marker when upgrading the same installation.
    } elseif ($owned -and ((& $normalize $owned) -eq $target)) {
        if ($present) {
            $next = @($entries | Where-Object { (& $normalize $_) -ne $target }) -join ';'
            $environment.SetValue('Path', $next, $kind)
        }
        $ownership.DeleteValue('OwnedPath', $false)
    }
} finally {
    $ownership.Close()
    $environment.Close()
}
