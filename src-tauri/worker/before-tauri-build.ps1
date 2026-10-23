$ErrorActionPreference = 'Stop'

& (Join-Path $PSScriptRoot 'verify-worker.ps1')
try {
  & (Join-Path $PSScriptRoot '../ocr-worker/verify-worker.ps1')
} catch {
  Write-Host "OCR worker needs rebuilding: $($_.Exception.Message)"
  & (Join-Path $PSScriptRoot '../ocr-worker/build-worker.ps1')
  & (Join-Path $PSScriptRoot '../ocr-worker/verify-worker.ps1')
}

& pnpm.cmd build
if ($LASTEXITCODE -ne 0) {
  throw "Frontend build failed (exit $LASTEXITCODE)."
}
