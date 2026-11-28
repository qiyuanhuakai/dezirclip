function New-FixtureWebViewEnvironment([int]$Port, [string]$UserDataDirectory) {
    if ($Port -lt 1024 -or $Port -gt 65535 -or [string]::IsNullOrWhiteSpace($UserDataDirectory)) { throw 'A fixture WebView port and private user-data directory are required.' }
    return @{ WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$Port"; WEBVIEW2_USER_DATA_FOLDER = $UserDataDirectory }
}

function Restore-FixtureWebViewEnvironmentEntry([string]$Name, [hashtable]$Prior) {
    $environmentPath = switch -Exact ($Name) {
        'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS' { 'Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS' }
        'WEBVIEW2_USER_DATA_FOLDER' { 'Env:WEBVIEW2_USER_DATA_FOLDER' }
        default { throw 'Only the two fixture WebView environment entries may be restored.' }
    }
    if ($Prior.existed -isnot [bool] -or ($Prior.existed -and $Prior.value -isnot [string])) { throw 'An original environment-entry presence and string value are required.' }
    if ($Prior.existed) {
        Set-Item -LiteralPath $environmentPath -Value $Prior.value -ErrorAction Stop
    } elseif (Test-Path -LiteralPath $environmentPath) {
        Remove-Item -LiteralPath $environmentPath -ErrorAction Stop
    }
    if ((Test-Path -LiteralPath $environmentPath) -ne $Prior.existed) { throw 'The original environment-entry presence was not restored.' }
    if ($Prior.existed -and (Get-Item -LiteralPath $environmentPath).Value -cne $Prior.value) { throw 'The original environment-entry value was not restored.' }
}
