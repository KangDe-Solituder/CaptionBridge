param([switch]$Dev, [switch]$WorkersOnly)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../build-support/worker-common.ps1')

Ensure-WorkerReady 'ASR' { & (Join-Path $PSScriptRoot 'verify-worker.ps1') } { & (Join-Path $PSScriptRoot 'build-worker.ps1') }
Ensure-WorkerReady 'OCR' { & (Join-Path $PSScriptRoot '../ocr-worker/verify-worker.ps1') } { & (Join-Path $PSScriptRoot '../ocr-worker/build-worker.ps1') }

if ($WorkersOnly) { return }
Push-Location (Join-Path $PSScriptRoot '../..')
try {
  if ($Dev) { & pnpm.cmd dev } else { & pnpm.cmd build }
  if ($LASTEXITCODE -ne 0) { throw "Frontend command failed (exit $LASTEXITCODE)." }
} finally { Pop-Location }
