$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../src-tauri/build-support/worker-common.ps1')

function Assert-True($Condition, [string]$Message) {
  if (-not $Condition) { throw "Assertion failed: $Message" }
}
function Assert-Throws([scriptblock]$Action, [string]$Message) {
  $Failed = $false
  try { & $Action } catch { $Failed = $true }
  Assert-True $Failed $Message
}

$TestParent = [IO.Path]::GetTempPath()
$TestRoot = Join-Path $TestParent ('livecaption-worker-tests-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $TestRoot | Out-Null
try {
  $Source = Join-Path $TestRoot 'worker.py'
  $Manifest = Join-Path $TestRoot 'worker-build.json'
  [IO.File]::WriteAllText($Source, "one`ntwo`n", [Text.UTF8Encoding]::new($false))
  Write-WorkerFingerprint $TestRoot $Manifest @('worker.py')
  Assert-WorkerFingerprint $TestRoot $Manifest @('worker.py')
  [IO.File]::SetLastWriteTimeUtc($Source, [DateTime]::UtcNow.AddDays(2))
  Assert-WorkerFingerprint $TestRoot $Manifest @('worker.py')
  [IO.File]::WriteAllText($Source, "one`r`ntwo`r`n", [Text.UTF8Encoding]::new($true))
  Assert-WorkerFingerprint $TestRoot $Manifest @('worker.py')
  Write-Host 'PASS: checkout timestamps, CRLF/LF and UTF-8 BOM do not invalidate unchanged source'

  [IO.File]::WriteAllText($Source, "one`ntwo changed`n", [Text.UTF8Encoding]::new($false))
  Assert-Throws { Assert-WorkerFingerprint $TestRoot $Manifest @('worker.py') } 'changed source must invalidate the bundle'
  Write-WorkerFingerprint $TestRoot $Manifest @('worker.py')
  Assert-WorkerFingerprint $TestRoot $Manifest @('worker.py')
  Assert-Throws { Assert-WorkerFingerprint $TestRoot $Manifest @('worker.py', 'missing.ps1') } 'new build inputs must be tracked'
  '{"schema":1,"files":{}}' | Set-Content -LiteralPath $Manifest -Encoding UTF8
  Assert-Throws { Assert-WorkerFingerprint $TestRoot $Manifest @('worker.py') } 'old manifests must rebuild once'
  '{broken' | Set-Content -LiteralPath $Manifest -Encoding UTF8
  Assert-Throws { Assert-WorkerFingerprint $TestRoot $Manifest @('worker.py') } 'corrupt manifests must rebuild'
  Assert-Throws { Assert-WorkerFingerprint $TestRoot (Join-Path $TestRoot 'missing.json') @('worker.py') } 'missing manifests must rebuild'
  Write-Host 'PASS: changed, added and missing inputs, old/corrupt/missing fingerprints are rejected'

  $script:Builds = 0; $script:Checks = 0
  Ensure-WorkerReady 'valid' { $script:Checks++ } { $script:Builds++ }
  Assert-True ($script:Builds -eq 0 -and $script:Checks -eq 1) 'valid bundle should be reused'
  $script:Checks = 0
  Ensure-WorkerReady 'stale' { $script:Checks++; if ($script:Checks -eq 1) { throw 'stale' } } { $script:Builds++ }
  Assert-True ($script:Builds -eq 1 -and $script:Checks -eq 2) 'stale bundle should rebuild once and verify again'
  $script:Checks = 0; $script:Builds = 0
  Assert-Throws { Ensure-WorkerReady 'broken' { $script:Checks++; throw 'protocol invalid' } { $script:Builds++ } } 'failed post-build verification must stop packaging'
  Assert-True ($script:Builds -eq 1 -and $script:Checks -eq 2) 'no infinite rebuild loop'
  $script:Checks = 0
  Assert-Throws { Ensure-WorkerReady 'build-failed' { $script:Checks++; throw 'missing exe' } { throw 'pip failed' } } 'build failure must propagate'
  Assert-True ($script:Checks -eq 1) 'never verify/package old output after a failed rebuild'
  Write-Host 'PASS: reuse, one-time rebuild, post-build validation and failure propagation'

  Assert-True (-not (Test-WorkerPython (Join-Path $TestRoot 'missing-python.exe') @(12))) 'missing Python must be rejected'
  Assert-Throws { Resolve-WorkerPython (Join-Path $TestRoot 'missing-python.exe') @(12) } 'explicit invalid Python must not silently fall back'
  $FixtureVenv = Join-Path $TestRoot '.venv-build'
  New-Item -ItemType Directory -Path $FixtureVenv | Out-Null
  'preserve' | Set-Content -LiteralPath (Join-Path $FixtureVenv 'marker.txt')
  Assert-Throws { Initialize-WorkerVenv $TestRoot (Join-Path $TestRoot 'missing-python.exe') @(12) } 'missing prerequisite must not delete existing environment'
  Assert-True (Test-Path -LiteralPath (Join-Path $FixtureVenv 'marker.txt')) 'environment should remain intact'
  Assert-Throws { Remove-WorkerGeneratedDirectory $TestRoot $TestRoot } 'cannot remove worker root'
  Assert-Throws { Remove-WorkerGeneratedDirectory $TestRoot ($TestRoot + '-sibling') } 'cannot remove sibling with matching prefix'
  Remove-WorkerGeneratedDirectory $TestRoot $FixtureVenv
  Assert-True (-not (Test-Path -LiteralPath $FixtureVenv)) 'generated child can be removed safely'
  Write-Host 'PASS: Python prerequisite errors preserve existing environments; cleanup stays within worker root'

  foreach ($Kind in @('asr', 'ocr')) {
    $Inputs = Get-WorkerInputs $Kind
    Assert-True ($Inputs -contains 'build-worker.ps1' -and $Inputs -contains '../build-support/worker-common.ps1') "$Kind tracks build logic changes"
  }
  Write-Host 'All worker build tests passed.'
} finally { Remove-WorkerGeneratedDirectory $TestParent $TestRoot }
