param(
    [Parameter(Mandatory=$true)][string]$Python,
    [string]$CudaBin = 'C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.4\bin',
    [string]$CudnnBin = ''
)
$ErrorActionPreference = 'Stop'
$WorkerDir = $PSScriptRoot
$VenvDir = Join-Path $WorkerDir '.venv-build'
& $Python -m venv $VenvDir
$VenvPython = Join-Path $VenvDir 'Scripts\python.exe'
& $VenvPython -m pip install --requirement (Join-Path $WorkerDir 'requirements.lock.txt')
& $VenvPython -m PyInstaller --noconfirm --clean --onedir --console --name livecaption-asr-worker `
  --collect-all faster_whisper --collect-all ctranslate2 --collect-all soundcard `
  (Join-Path $WorkerDir 'asr_worker.py')
$InternalDir = Join-Path $WorkerDir 'dist\livecaption-asr-worker\_internal'
$CudaFiles = @('cublas64_12.dll', 'cublasLt64_12.dll')
foreach ($File in $CudaFiles) {
  $Source = Join-Path $CudaBin $File
  if (-not (Test-Path -LiteralPath $Source)) { throw "Missing CUDA runtime: $Source" }
  Copy-Item -LiteralPath $Source -Destination $InternalDir -Force
}
if (-not $CudnnBin) {
  $Candidates = @(
    'C:\Program Files\NVIDIA\CUDNN\v9.0\bin\12.4',
    'C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.4\bin'
  )
  $CudnnBin = $Candidates | Where-Object { Test-Path -LiteralPath (Join-Path $_ 'cudnn64_9.dll') } | Select-Object -First 1
}
if (-not $CudnnBin -or -not (Test-Path -LiteralPath (Join-Path $CudnnBin 'cudnn64_9.dll'))) {
  throw 'cuDNN 9 runtime not found. Pass -CudnnBin pointing to the directory containing cudnn64_9.dll.'
}
Get-ChildItem -LiteralPath $CudnnBin -Filter 'cudnn*64_9.dll' | Copy-Item -Destination $InternalDir -Force
Write-Host "Worker built at $WorkerDir\dist\livecaption-asr-worker\livecaption-asr-worker.exe"
