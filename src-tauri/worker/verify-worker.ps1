param(
  [string]$WorkerExe = (Join-Path $PSScriptRoot 'dist\livecaption-asr-worker\livecaption-asr-worker.exe')
)

$ErrorActionPreference = 'Stop'

function Get-Sha256Hex([string]$FilePath) {
  $FileStream = [System.IO.File]::OpenRead($FilePath)
  try {
    $Sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
      return ([System.BitConverter]::ToString($Sha256.ComputeHash($FileStream))).Replace('-', '').ToLowerInvariant()
    } finally {
      $Sha256.Dispose()
    }
  } finally {
    $FileStream.Dispose()
  }
}

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
foreach ($SourceName in @('asr_worker.py', 'requirements.lock.txt', 'build-worker.ps1')) {
  $SourcePath = Join-Path $PSScriptRoot $SourceName
  $ExpectedHash = $BuildManifest.files.$SourceName
  $ActualHash = Get-Sha256Hex $SourcePath
  if (-not $ExpectedHash -or $ExpectedHash.ToLowerInvariant() -ne $ActualHash) {
    throw "ASR Worker is stale because $SourceName changed after it was built. Rebuild the Worker before packaging the app."
  }
}

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
