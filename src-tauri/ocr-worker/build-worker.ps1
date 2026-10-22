$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'hash-file.ps1')
$python = Join-Path $PSScriptRoot '.venv-build/Scripts/python.exe'
if (-not (Test-Path $python)) {
  & python -m venv (Join-Path $PSScriptRoot '.venv-build')
  if ($LASTEXITCODE -ne 0) { throw 'Cannot create OCR build environment' }
}
& $python -m pip install -r (Join-Path $PSScriptRoot 'requirements.lock.txt')
if ($LASTEXITCODE -ne 0) { throw 'Cannot install OCR dependencies' }
Push-Location $PSScriptRoot
try {
  Copy-Item 'worker.spec.template' 'worker.spec' -Force
  & $python -m PyInstaller --noconfirm --clean worker.spec
  if ($LASTEXITCODE -ne 0) { throw 'Cannot package OCR worker' }
  $fingerprint = @{}
  foreach ($name in @('ocr_worker.py', 'requirements.lock.txt', 'worker.spec.template')) {
    $fingerprint[$name] = Get-OcrFileHash (Join-Path $PSScriptRoot $name)
  }
  $fingerprint | ConvertTo-Json | Set-Content 'dist/livecaption-ocr-worker/worker-build.json'
} finally { Pop-Location }
