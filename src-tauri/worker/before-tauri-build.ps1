$ErrorActionPreference = 'Stop'

& (Join-Path $PSScriptRoot 'verify-worker.ps1')

& pnpm.cmd build
if ($LASTEXITCODE -ne 0) {
  throw "Frontend build failed (exit $LASTEXITCODE)."
}
