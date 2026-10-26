# Shared by ASR/OCR packaging. Keep paths and interpreter discovery machine-local.
function Get-WorkerSourceHash([string]$Path) {
  # Git may check text out as CRLF or LF; neither BOM nor EOL affects Python source.
  $Text = [IO.File]::ReadAllText($Path, [Text.Encoding]::UTF8).Replace("`r`n", "`n")
  $Sha = [Security.Cryptography.SHA256]::Create()
  try {
    $Bytes = ([Text.UTF8Encoding]::new($false)).GetBytes($Text)
    return ([BitConverter]::ToString($Sha.ComputeHash($Bytes))).Replace('-', '').ToLowerInvariant()
  } finally { $Sha.Dispose() }
}

function Write-WorkerFingerprint([string]$WorkerDir, [string]$ManifestPath, [string[]]$Inputs) {
  $Hashes = [ordered]@{}
  foreach ($Name in $Inputs) { $Hashes[$Name] = Get-WorkerSourceHash (Join-Path $WorkerDir $Name) }
  @{ schema = 2; files = $Hashes } | ConvertTo-Json -Depth 3 |
    Set-Content -LiteralPath $ManifestPath -Encoding UTF8
}

function Assert-WorkerFingerprint([string]$WorkerDir, [string]$ManifestPath, [string[]]$Inputs) {
  if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) { throw 'Worker build fingerprint is missing.' }
  $Manifest = Get-Content -LiteralPath $ManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
  if ($Manifest.schema -ne 2 -or -not $Manifest.files) { throw 'Worker build fingerprint needs upgrading.' }
  foreach ($Name in $Inputs) {
    $Expected = $Manifest.files.$Name
    $Actual = Get-WorkerSourceHash (Join-Path $WorkerDir $Name)
    if (-not $Expected -or $Expected -ne $Actual) { throw "Worker is stale: $Name changed since the last build." }
  }
}

function Get-WorkerInputs([ValidateSet('asr', 'ocr')][string]$Kind) {
  $Common = @('requirements.lock.txt', 'build-worker.ps1', '../build-support/worker-common.ps1')
  if ($Kind -eq 'asr') { return @('asr_worker.py', 'gpu-runtime-requirements.lock.txt') + $Common }
  return @('ocr_worker.py', 'worker.spec.template') + $Common
}

function Test-WorkerPython([string]$Python, [int[]]$MinorVersions) {
  if (-not $Python -or $Python -match '\\WindowsApps\\') { return $false }
  $ErrorActionPreference = 'SilentlyContinue'
  try {
    $Result = & $Python -I -c 'import sys, struct; print(sys.version_info.major, sys.version_info.minor, struct.calcsize(chr(80)) * 8)' 2>$null
    return $LASTEXITCODE -eq 0 -and @($MinorVersions | ForEach-Object { "3 $_ 64" }) -contains "$Result".Trim()
  } catch { return $false }
}

function Resolve-WorkerPython([string]$Python, [int[]]$MinorVersions) {
  if (-not $Python) { $Python = $env:LIVECAPTION_BUILD_PYTHON }
  if ($Python) {
    if (Test-WorkerPython $Python $MinorVersions) { return $Python }
    throw "Invalid build Python: $Python. Required: Python $($MinorVersions.ForEach({ "3.$_" }) -join ' or ') x64."
  }
  foreach ($Minor in $MinorVersions) {
    $Candidates = @()
    if (Get-Command py -ErrorAction SilentlyContinue) {
      # A missing launcher version writes to stderr on Windows PowerShell 5.1.
      $PreviousPreference = $ErrorActionPreference
      try {
        $ErrorActionPreference = 'SilentlyContinue'
        $Found = & py "-3.$Minor" -c 'import sys; print(sys.executable)' 2>$null
        if ($LASTEXITCODE -eq 0) { $Candidates += "$Found".Trim() }
      } finally { $ErrorActionPreference = $PreviousPreference }
    }
    if ($env:APPDATA) {
      $Candidates += @(Get-ChildItem -Path (Join-Path $env:APPDATA "uv/python/cpython-3.$Minor*-windows-x86_64-*/python.exe") -ErrorAction SilentlyContinue | Sort-Object FullName -Descending | ForEach-Object { $_.FullName })
    }
    if ($env:LOCALAPPDATA) { $Candidates += Join-Path $env:LOCALAPPDATA "Programs/Python/Python3$Minor/python.exe" }
    $Candidates += 'python'
    foreach ($Candidate in $Candidates) {
      if (Test-WorkerPython $Candidate @($Minor)) { return $Candidate }
    }
  }
  throw 'No supported x64 build Python found. Install Python 3.12 x64, or set LIVECAPTION_BUILD_PYTHON to its python.exe path, then retry. No build paths need to be committed to Git.'
}

function Remove-WorkerGeneratedDirectory([string]$WorkerDir, [string]$Path) {
  $Root = [IO.Path]::GetFullPath($WorkerDir).TrimEnd('\', '/')
  $Target = [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
  if (-not $Target.StartsWith($Root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to clean outside worker directory: $Target"
  }
  # Do not follow a junction/symlink into another checkout or Python installation.
  $Ancestor = $Target
  while ($Ancestor -and $Ancestor -ne $Root) {
    if (Test-Path -LiteralPath $Ancestor) {
      if ((Get-Item -LiteralPath $Ancestor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Refusing to clean linked directory: $Ancestor" }
    }
    $Ancestor = Split-Path $Ancestor -Parent
  }
  if (Test-Path -LiteralPath $Target) { Remove-Item -LiteralPath $Target -Recurse -Force }
}

function Initialize-WorkerVenv([string]$WorkerDir, [string]$Python, [int[]]$MinorVersions) {
  $VenvDir = Join-Path $WorkerDir '.venv-build'
  $VenvPython = Join-Path $VenvDir 'Scripts/python.exe'
  if (Test-WorkerPython $VenvPython $MinorVersions) {
    Write-Host "Reusing build environment at $VenvDir"
    return $VenvPython
  }
  # Resolve first: missing prerequisites must not destroy an existing environment.
  $BasePython = Resolve-WorkerPython $Python $MinorVersions
  Remove-WorkerGeneratedDirectory $WorkerDir $VenvDir
  & $BasePython -m venv $VenvDir | Out-Host
  if ($LASTEXITCODE -ne 0 -or -not (Test-WorkerPython $VenvPython $MinorVersions)) { throw "Cannot create worker build environment at $VenvDir" }
  return $VenvPython
}

function Ensure-WorkerReady([string]$Name, [scriptblock]$Verify, [scriptblock]$Build) {
  try { & $Verify; return } catch { Write-Host "$Name worker needs rebuilding: $($_.Exception.Message)" }
  # Rebuild once, then require verification. Never package stale output on failure.
  & $Build
  & $Verify
}
