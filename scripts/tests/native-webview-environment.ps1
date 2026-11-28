$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSVersion -lt [version]'7.5') { throw 'PowerShell 7.5 is required to distinguish empty and absent process environment values.' }
. (Join-Path $PSScriptRoot '../native-windows-webview-environment.ps1')

function Assert-Throws([scriptblock]$Command, [string]$Message) {
    try { & $Command }
    catch {
        if ($_.Exception.Message -ceq $Message) { return }
        throw
    }
    throw "Expected an error: $Message"
}

$directory = Join-Path ([IO.Path]::GetTempPath()) 'synthetic WebView environment'
$overrides = New-FixtureWebViewEnvironment 9222 $directory
if ($overrides.Count -ne 2 -or $overrides.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -cne '--remote-debugging-port=9222' -or
    $overrides.WEBVIEW2_USER_DATA_FOLDER -cne $directory) { throw 'Incorrect process overrides.' }
Assert-Throws { New-FixtureWebViewEnvironment 443 $directory } 'A fixture WebView port and private user-data directory are required.'
Assert-Throws { Restore-FixtureWebViewEnvironmentEntry PATH @{ existed = $false } } 'Only the two fixture WebView environment entries may be restored.'

foreach ($name in $overrides.Keys) {
    $path = "Env:$name"
    $present = Test-Path -LiteralPath $path
    $original = @{ existed = $present; value = $(if ($present) { (Get-Item -LiteralPath $path).Value }) }
    $absent = @{ existed = $false; value = $null }
    $presentText = @{ existed = $true; value = 'original text' }
    try {
        foreach ($state in @($absent, @{ existed = $true; value = '' }, $presentText)) {
            foreach ($initial in @($absent, @{ existed = $true; value = $overrides[$name] })) {
                Restore-FixtureWebViewEnvironmentEntry $name $initial
                Restore-FixtureWebViewEnvironmentEntry $name $state
                if ((Test-Path -LiteralPath $path) -ne $state.existed -or
                    ($state.existed -and (Get-Item -LiteralPath $path).Value -cne $state.value)) {
                    throw 'The process environment differs from the original presence or value.'
                }
            }
        }
        Assert-Throws { Restore-FixtureWebViewEnvironmentEntry $name @{ existed = $true; value = $null } } 'An original environment-entry presence and string value are required.'
        & {
            function Set-Item { throw 'Synthetic set failure' }
            function Remove-Item { throw 'Synthetic remove failure' }
            Assert-Throws { Restore-FixtureWebViewEnvironmentEntry $name $presentText } 'Synthetic set failure'
            Assert-Throws { Restore-FixtureWebViewEnvironmentEntry $name $absent } 'Synthetic remove failure'
        }
        & {
            function Set-Item {}
            function Remove-Item {}
            Assert-Throws { Restore-FixtureWebViewEnvironmentEntry $name @{ existed = $true; value = 'different' } } 'The original environment-entry value was not restored.'
            Assert-Throws { Restore-FixtureWebViewEnvironmentEntry $name $absent } 'The original environment-entry presence was not restored.'
        }
        if ((Get-Item -LiteralPath $path).Value -cne $presentText.value) { throw 'A failed restoration changed the current process environment.' }
    } finally {
        Restore-FixtureWebViewEnvironmentEntry $name $original
    }
}
Write-Output 'Verified WebView environment restoration using the real PowerShell Env provider.'
