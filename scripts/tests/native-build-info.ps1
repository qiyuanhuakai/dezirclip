$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '../test-native-windows-upgrade.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw ($errors | Out-String) }
$functions = @($ast.FindAll({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Convert-GuiConfigurationJson' }, $true))
if ($functions.Count -ne 1) { throw 'Missing GUI configuration serializer.' }
# Import only the serializer. Installer/process/registry statements never run.
. ([scriptblock]::Create($functions[0].Extent.Text))
$manifest = [pscustomobject]@{ version = '0.2.0-alpha.1'; buildInfo = [pscustomobject]@{
    component = 'cli'; metadata_version = 1; component_version = '0.2.0-alpha.1'
    protocol_version = [pscustomobject]@{ major = 1; minor = 0 }; database_schema_version = 21
    linked_components = @([pscustomobject]@{ component = 'dzc-core'; version = '0.2.0-alpha.1' }, [pscustomobject]@{ component = 'dzc-platform'; version = '0.2.0-alpha.1' }, [pscustomobject]@{ component = 'dzc-protocol'; version = '0.2.0-alpha.1' })
} }
$configuration = Convert-GuiConfigurationJson $manifest 'C:\synthetic\profile' 'synthetic-marker' $false $true | ConvertFrom-Json
if ($configuration.expectedBuildInfo.database_schema_version -ne 21) { throw 'Current database schema metadata was truncated.' }
if ($configuration.expectedBuildInfo.protocol_version.major -ne 1 -or $configuration.expectedBuildInfo.protocol_version.minor -ne 0) { throw 'Protocol object was truncated.' }
if ($configuration.expectedBuildInfo.linked_components.Count -ne 3 -or $configuration.expectedBuildInfo.linked_components[0] -is [string] -or $configuration.expectedBuildInfo.linked_components[0].component -ne 'dzc-core' -or $configuration.expectedBuildInfo.linked_components[2].version -ne $manifest.version) { throw 'Nested component metadata was truncated.' }
if ($configuration.acquireOwnership -ne $false -or $configuration.verifyCapture -ne $true -or $configuration.expectedVersion -ne $manifest.version) { throw 'GUI fixture inputs changed during serialization.' }
Write-Output 'GUI build-info configuration preserves nested protocol/components and fixture values.'
