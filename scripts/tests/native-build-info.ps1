$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '../test-native-windows-upgrade.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
# Import pure declarations only; installer/process/registry statements never run.
foreach ($name in @('Convert-GuiConfigurationJson', 'Assert-CurrentBuildInfo')) {
    $functions = @($ast.FindAll({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true))
    if ($functions.Count -ne 1) { throw "Missing build metadata function: $name" }
    . ([scriptblock]::Create($functions[0].Extent.Text))
}
function New-SyntheticBuildInfo([string]$Role) {
    $names = @('dzc-core', 'dzc-platform', 'dzc-protocol', 'dzc-host')
    if ($Role -eq 'service') { $names += 'dzc-service' }
    return [pscustomobject][ordered]@{
        component = $Role; metadata_version = 1; component_version = '0.2.0-alpha.2'
        protocol_version = [pscustomobject]@{ major = 1; minor = 0 }; database_schema_version = 24
        source_revision = ('a' * 40); build_id = ('0.2.0-alpha.2@' + ('a' * 40))
        source_state = 'modified_checkout'; target = 'x86_64-pc-windows-msvc'
        linked_components = @($names | ForEach-Object { [pscustomobject]@{ component = $_; version = '0.2.0-alpha.2' } })
    }
}
function Assert-RejectedBuild($Info, [string]$Role) {
    $rejected = $false
    try { Assert-CurrentBuildInfo $Info $Role '0.2.0-alpha.2' ('a' * 40) } catch { $rejected = $true }
    if (-not $rejected) { throw 'Malformed build metadata was accepted.' }
}
foreach ($role in @('cli', 'desktop', 'service')) {
    Assert-CurrentBuildInfo (New-SyntheticBuildInfo $role) $role '0.2.0-alpha.2' ('a' * 40)
    foreach ($change in @(
        @('database_schema_version', 23), @('database_schema_version', 25), @('database_schema_version', $true), @('database_schema_version', '24'),
        @('metadata_version', $true), @('metadata_version', 2), @('component', 'surface'),
        @('source_state', 'unknown'), @('source_revision', ('b' * 40)), @('build_id', 'unrelated'),
        @('target', 'aarch64-pc-windows-msvc'), @('linked_components', @())
    )) {
        $info = New-SyntheticBuildInfo $role
        $info.($change[0]) = $change[1]
        Assert-RejectedBuild $info $role
    }
    $info = New-SyntheticBuildInfo $role
    $info.protocol_version.major = $true
    Assert-RejectedBuild $info $role
    $info = New-SyntheticBuildInfo $role
    $info.linked_components[0].version = '0.2.0-alpha.1'
    Assert-RejectedBuild $info $role
    $info = New-SyntheticBuildInfo $role
    $info.linked_components[-1].component = 'dzc-core'
    Assert-RejectedBuild $info $role
}
$manifest = [pscustomobject]@{ version = '0.2.0-alpha.2'; buildInfo = (New-SyntheticBuildInfo 'cli') }
$configuration = Convert-GuiConfigurationJson $manifest 'C:\synthetic\profile' 'synthetic-marker' $false $true | ConvertFrom-Json
if ($configuration.expectedBuildInfo.database_schema_version -ne 24) { throw 'Current database schema metadata was truncated.' }
if ($configuration.expectedBuildInfo.protocol_version.major -ne 1 -or $configuration.expectedBuildInfo.protocol_version.minor -ne 0) { throw 'Protocol object was truncated.' }
if ($configuration.expectedBuildInfo.linked_components.Count -ne 4 -or $configuration.expectedBuildInfo.linked_components[0] -is [string] -or $configuration.expectedBuildInfo.linked_components[0].component -ne 'dzc-core' -or $configuration.expectedBuildInfo.linked_components[3].version -ne $manifest.version) { throw 'Nested component metadata was truncated.' }
if ($configuration.acquireOwnership -ne $false -or $configuration.verifyCapture -ne $true -or $configuration.expectedVersion -ne $manifest.version) { throw 'GUI fixture inputs changed during serialization.' }
Write-Output 'Independent alpha.2 schema24/component policy rejects mixed, unknown, mistyped and obsolete metadata; GUI serialization retains the full inventory.'
