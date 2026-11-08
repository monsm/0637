# Stage the official MJCF, STL meshes and ONNX policies into the iOS bundle.
$ErrorActionPreference = 'Stop'

$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$src  = Join-Path $root 'upstream\microduck_rl\src\mjlab_microduck\robot\microduck'
$dst  = Join-Path $root 'ios\DuckApp\Resources'

if (-not (Test-Path $src))  { throw "missing $src - clone microduck_rl into upstream/ first" }
if (-not (Test-Path (Join-Path $root 'policies'))) { throw "missing policies/ - download the official ONNX set first" }

if (Test-Path $dst) { Remove-Item $dst -Recurse -Force -Confirm:$false }
New-Item -ItemType Directory -Force (Join-Path $dst 'robot\microduck\assets'), (Join-Path $dst 'policies') | Out-Null

Copy-Item (Join-Path $src '*.xml') (Join-Path $dst 'robot\microduck') -Force
Copy-Item (Join-Path $src 'assets\*.stl') (Join-Path $dst 'robot\microduck\assets') -Force
Copy-Item (Join-Path $root 'policies\*.onnx') (Join-Path $dst 'policies') -Force
$manifest = Join-Path $root 'policies\manifest.json'
if (Test-Path $manifest) { Copy-Item $manifest (Join-Path $dst 'policies') -Force }

$size = (Get-ChildItem $dst -Recurse -File | Measure-Object Length -Sum).Sum
"staged resources into $dst ($([math]::Round($size/1MB,1)) MB)"
