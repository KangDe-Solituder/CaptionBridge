param(
  [string]$WorkerExe = (Join-Path $PSScriptRoot 'dist\livecaption-asr-worker\livecaption-asr-worker.exe')
)

$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot '../build-support/worker-common.ps1')

if (-not (Test-Path -LiteralPath $WorkerExe -PathType Leaf)) {
  throw "ASR Worker not found at $WorkerExe. Run build-worker.ps1 before packaging the app."
}

$BuildManifestPath = Join-Path (Split-Path $WorkerExe -Parent) 'worker-build.json'
Assert-WorkerFingerprint $PSScriptRoot $BuildManifestPath (Get-WorkerInputs 'asr')

$ProtocolOutput = @(
  '{"command":"check_python_runtime"}',
  '{"command":"configure","vad":{"channel_mode":"auto"}}',
  '{"command":"reset_routing"}',
  '{"command":"probe_dependencies"}',
  '{"command":"shutdown"}'
) | & $WorkerExe

$ProtocolText = $ProtocolOutput -join "`n"
$HasProtocolError = $ProtocolText -match '"type"\s*:\s*"error"'
$HasDependencyProbe = $ProtocolText -match '"type"\s*:\s*"dependency_probe"'
$HasPythonRuntime = $ProtocolText -match '"detail"\s*:\s*"python_runtime_ok"'

if ($LASTEXITCODE -ne 0 -or $HasProtocolError -or -not $HasDependencyProbe -or -not $HasPythonRuntime) {
  throw @"
ASR Worker protocol verification failed. The executable is stale or incompatible with the app.
Run src-tauri\worker\build-worker.ps1, then retry the Tauri build.

$ProtocolText
"@
}

Write-Host "ASR Worker protocol verified at $WorkerExe"
