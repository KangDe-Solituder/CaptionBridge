param(
  [string]$WorkerExe = (Join-Path $PSScriptRoot 'dist\livecaption-asr-worker\livecaption-asr-worker.exe')
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $WorkerExe -PathType Leaf)) {
  throw "ASR Worker not found at $WorkerExe. Run build-worker.ps1 before packaging the app."
}

$BuildManifestPath = Join-Path (Split-Path $WorkerExe -Parent) 'worker-build.json'
if (-not (Test-Path -LiteralPath $BuildManifestPath -PathType Leaf)) {
  throw "ASR Worker build fingerprint is missing. Rebuild the Worker before packaging the app."
}
$BuildManifest = Get-Content -LiteralPath $BuildManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
if ($BuildManifest.schema -ne 1 -or -not $BuildManifest.files) {
  throw "ASR Worker build fingerprint is invalid. Rebuild the Worker before packaging the app."
}
foreach ($SourceName in @('asr_worker.py', 'requirements.lock.txt')) {
  $SourcePath = Join-Path $PSScriptRoot $SourceName
  $ExpectedHash = $BuildManifest.files.$SourceName
  $ActualHash = (Get-FileHash -LiteralPath $SourcePath -Algorithm SHA256).Hash.ToLowerInvariant()
  if (-not $ExpectedHash -or $ExpectedHash.ToLowerInvariant() -ne $ActualHash) {
    throw "ASR Worker is stale because $SourceName changed after it was built. Rebuild the Worker before packaging the app."
  }
}

$ProtocolOutput = @(
  '{"command":"configure","vad":{"channel_mode":"auto"}}',
  '{"command":"reset_routing"}',
  '{"command":"probe_dependencies"}',
  '{"command":"shutdown"}'
) | & $WorkerExe

$ProtocolText = $ProtocolOutput -join "`n"
$HasProtocolError = $ProtocolText -match '"type"\s*:\s*"error"'
$HasDependencyProbe = $ProtocolText -match '"type"\s*:\s*"dependency_probe"'

if ($LASTEXITCODE -ne 0 -or $HasProtocolError -or -not $HasDependencyProbe) {
  throw @"
ASR Worker protocol verification failed. The executable is stale or incompatible with the app.
Run src-tauri\worker\build-worker.ps1, then retry the Tauri build.

$ProtocolText
"@
}

Write-Host "ASR Worker protocol verified at $WorkerExe"
