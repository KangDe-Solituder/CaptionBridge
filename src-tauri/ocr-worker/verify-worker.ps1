$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'hash-file.ps1')
$workerDir = Join-Path $PSScriptRoot 'dist/livecaption-ocr-worker'
if (-not (Test-Path (Join-Path $workerDir 'livecaption-ocr-worker.exe'))) {
  throw 'OCR worker missing. Run src-tauri/ocr-worker/build-worker.ps1 before packaging.'
}
$manifestPath = Join-Path $workerDir 'worker-build.json'
if (-not (Test-Path $manifestPath)) { throw 'OCR build fingerprint missing. Rebuild OCR worker.' }
$manifest = Get-Content $manifestPath -Raw | ConvertFrom-Json
foreach ($name in @('ocr_worker.py', 'requirements.lock.txt', 'worker.spec.template')) {
  $hash = Get-OcrFileHash (Join-Path $PSScriptRoot $name)
  if ($manifest.$name -ne $hash) { throw "OCR worker stale: $name changed. Rebuild OCR worker." }
}
$responses = @('{"command":"ping"}' | & (Join-Path $workerDir 'livecaption-ocr-worker.exe'))
if ($LASTEXITCODE -ne 0 -or $responses.Count -ne 1) { throw 'OCR worker did not return one ping response' }
$responses | ForEach-Object {
  $response = $_ | ConvertFrom-Json
  if (-not $response.ok -or $response.protocol -ne 1) { throw 'OCR worker protocol mismatch' }
}
if ($LASTEXITCODE -ne 0) { throw 'OCR worker cannot start' }
Write-Host 'OCR worker verified.'
