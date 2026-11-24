# SPDX-License-Identifier: AGPL-3.0-or-later
#Requires -Version 7.4

[CmdletBinding(PositionalBinding = $false)]
param(
    [Parameter(Position = 0)][string]$Kind,
    [Parameter(Position = 1)][string]$X,
    [Parameter(Position = 2)][string]$Y,
    [string]$Cache,
    [double]$Scale = 0.68,
    [string]$Name = '2009scape',
    [string]$Output = (Join-Path $PSScriptRoot 'output'),
    [string]$StudioMdl = 'C:/Program Files (x86)/Steam/steamapps/common/GarrysMod/bin/win64/studiomdl.exe',
    [string]$Addon = 'C:/Program Files (x86)/Steam/steamapps/common/GarrysMod/garrysmod/addons/garryscape',
    [switch]$PreviewOnly,
    [switch]$Rebuild,
    [Alias('h')][switch]$Help
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$PSNativeCommandArgumentPassing = 'Standard'

function Get-FullPath {
    param([string]$Path)

    $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
}

function Resolve-LinkedPath {
    param([string]$Path)

    $root = [IO.Path]::GetPathRoot($Path)
    $resolved = $root
    foreach ($part in ($Path.Substring($root.Length) -split '[/\\]')) {
        if (!$part) { continue }
        $resolved = Join-Path $resolved $part
        if (Test-Path -LiteralPath $resolved) {
            $item = Get-Item -LiteralPath $resolved -Force
            $target = $item.ResolveLinkTarget($true)
            if ($null -ne $target) { $resolved = $target.FullName }
        }
    }
    $resolved
}

function Copy-Asset {
    param([string]$Source, [string]$Destination)

    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Destination))
    [IO.File]::Copy($Source, $Destination)
}

if ($Help -or !$PSBoundParameters.Count) {
    @'
Usage: ./convert.ps1 regions X Y [options]
  -Cache PATH       Native 2009Scape cache (default: $env:USERPROFILE/2009scape/cache/runescape)
  -Scale NUMBER     Model scale (default: 0.68)
  -Name NAME        Namespace (default: 2009scape; empty string uses 530)
  -Output PATH      Build and preview directory (default: output beside this script)
  -StudioMdl PATH   GMod's bin/win64/studiomdl.exe
  -Addon PATH       Compiled model and material destination
  -PreviewOnly      Export glTF, DMX, QC and materials without compiling or installing
  -Rebuild          Regenerate converted assets
Only terrain is implemented. Region coordinates are required. No collision.
'@
    exit 0
}

try {
    if ($Kind -cne 'regions') { throw 'Only regions are implemented.' }
    if ($X -notmatch '^[0-9]{1,3}$' -or $Y -notmatch '^[0-9]{1,3}$') {
        throw 'Specify the region: ./convert.ps1 regions 50 50'
    }
    if ([int]$X -gt 255 -or [int]$Y -gt 255) { throw 'Region coordinates must be 0..255.' }
    $X = [int]$X
    $Y = [int]$Y
    if (!$Name) { $Name = '530' }
    if ($Name -cnotmatch '^[a-z0-9_-]+$') {
        throw 'Namespace must use lowercase letters, digits, hyphens or underscores.'
    }
    if (!$PSBoundParameters.ContainsKey('Cache') -and $env:USERPROFILE) {
        $Cache = Join-Path $env:USERPROFILE '2009scape/cache/runescape'
    }
    if (!$Cache) { throw 'Set USERPROFILE or pass -Cache PATH.' }
    $Cache = Get-FullPath $Cache
    $Output = Get-FullPath $Output
    $cacheFiles = @(
        'main_file_cache.dat2'
        'main_file_cache.dat2.dat'
        'LIVE/main_file_cache.dat2'
        'LIVE/main_file_cache.dat2.dat'
    )
    if (!($cacheFiles | Where-Object { Test-Path -LiteralPath (Join-Path $Cache $_) -PathType Leaf })) {
        throw "2009Scape cache not found in $Cache. Supply your revision-530 cache with -Cache."
    }
    if (!$PreviewOnly) {
        $StudioMdl = Get-FullPath $StudioMdl
        if (!(Test-Path -LiteralPath $StudioMdl -PathType Leaf)) {
            throw "StudioMDL not found at $StudioMdl. Use -PreviewOnly to export without it."
        }
    }
    $cargo = Get-Command cargo -CommandType Application -ErrorAction SilentlyContinue
    if (!$cargo) { throw 'Nightly Rust/Cargo is required. Install it from https://rustup.rs.' }
    [void][IO.Directory]::CreateDirectory($Output)

    $flagName = 'RUSTFLAGS'
    $separator = ' '
    if (Test-Path Env:CARGO_ENCODED_RUSTFLAGS) {
        $flagName = 'CARGO_ENCODED_RUSTFLAGS'
        $separator = [string][char]31
    }
    $previousFlags = [Environment]::GetEnvironmentVariable($flagName)
    Push-Location -LiteralPath $PSScriptRoot
    try {
        $flags = if ($previousFlags) { "$previousFlags$separator-Ctarget-cpu=native" } else { '-Ctarget-cpu=native' }
        [Environment]::SetEnvironmentVariable($flagName, $flags)
        & $cargo.Source build --locked --release --manifest-path (Join-Path $PSScriptRoot 'converter/Cargo.toml')
        if ($LASTEXITCODE -ne 0) { throw "Cargo build failed (exit $LASTEXITCODE)." }
    }
    finally {
        [Environment]::SetEnvironmentVariable($flagName, $previousFlags)
        Pop-Location
    }

    $cli = Join-Path $PSScriptRoot 'converter/target/release/garryscape-convert'
    if ($IsWindows) { $cli += '.exe' }
    $scaleArg = $Scale.ToString([Globalization.CultureInfo]::InvariantCulture)
    $converterArgs = @('regions', $X, $Y, '--cache', $Cache, '--scale', $scaleArg, '--name', $Name, '--output', $Output)
    if ($Rebuild) { $converterArgs += '--rebuild' }
    & $cli @converterArgs
    if ($LASTEXITCODE -ne 0) { throw "Conversion failed (exit $LASTEXITCODE)." }

    $id = "${X}_${Y}"
    $region = "$Name/regions/$id"
    $build = Join-Path $Output $region
    if ($PreviewOnly) {
        "Preview ready: $(Join-Path $build "preview/$id.gltf")"
        exit 0
    }

    if (!$Addon) { throw 'Addon destination must not be empty.' }
    if ($Addon -match '(^|[/\\])\.{1,2}[/\\]*$') { throw 'Unsafe addon destination.' }
    $Addon = [IO.Path]::TrimEndingDirectorySeparator((Get-FullPath $Addon))
    if ($Addon -eq [IO.Path]::GetPathRoot($Addon)) { throw 'Unsafe addon destination.' }
    $existing = Get-Item -LiteralPath $Addon -Force -ErrorAction SilentlyContinue
    if ($existing -and ($existing.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'Addon destination must not be a symbolic link or junction.'
    }
    $canonicalAddon = Resolve-LinkedPath $Addon
    foreach ($protected in $PSScriptRoot, $Output, $Cache) {
        $canonicalProtected = Resolve-LinkedPath $protected
        $separator = [IO.Path]::DirectorySeparatorChar
        $addonPrefix = $canonicalAddon.TrimEnd($separator) + $separator
        $protectedPrefix = $canonicalProtected.TrimEnd($separator) + $separator
        $comparison = if ($IsWindows) { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
        $overlap = $protectedPrefix.StartsWith($addonPrefix, $comparison) -or
        $addonPrefix.StartsWith($protectedPrefix, $comparison)
        if ($overlap) {
            throw "Addon destination would delete source, build or cache files: $Addon"
        }
    }

    "Addon destination: $Addon"
    $source = Join-Path $build "source/garryscape/$region"
    $game = Join-Path $build 'game'
    $model = Join-Path $game "models/garryscape/$region"
    $required = '.mdl', '.vvd', '.dx90.vtx'
    $suffixes = $required + '.dx80.vtx', '.sw.vtx'
    $complete = Join-Path $build 'complete.json'
    $compiled = Join-Path $build 'compiled.json'
    $toolMarker = Join-Path $build 'compiled-tool.txt'
    $toolHash = (Get-FileHash -LiteralPath $StudioMdl).Hash
    $validOutputs = !($required | Where-Object {
            $asset = Get-Item -LiteralPath "$model$_" -ErrorAction SilentlyContinue
            !$asset -or $asset.PSIsContainer -or !$asset.Length
        })
    $reuse = !$Rebuild -and $validOutputs -and [IO.File]::Exists($toolMarker) -and [IO.File]::Exists($compiled) -and
    $toolHash -ceq [IO.File]::ReadAllText($toolMarker).Trim() -and
    (Get-FileHash -LiteralPath $complete).Hash -ceq (Get-FileHash -LiteralPath $compiled).Hash
    if (!$reuse) {
        $qc = Join-Path $source 'model.qc'
        $log = Join-Path $source 'compile.log'
        "Compiling $qc"
        [IO.File]::Delete($compiled)
        [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($model))
        foreach ($suffix in $suffixes) { [IO.File]::Delete("$model$suffix") }
        [IO.File]::WriteAllText($log, '')
        Push-Location -LiteralPath $source
        try {
            & $StudioMdl -game $game $qc 2>&1 | Tee-Object -LiteralPath $log
            if ($LASTEXITCODE -ne 0) { throw "Compilation failed. Inputs and log retained at $source" }
        }
        finally {
            Pop-Location
        }
        foreach ($suffix in $required) {
            $asset = Get-Item -LiteralPath "$model$suffix" -ErrorAction SilentlyContinue
            if (!$asset -or $asset.PSIsContainer -or !$asset.Length) {
                throw "StudioMDL did not produce $model$suffix. See $log."
            }
        }
        if (Select-String -LiteralPath $log -Pattern 'ERROR:' -SimpleMatch -Quiet) {
            throw "StudioMDL reported errors. See $log."
        }
        [IO.File]::Copy($complete, $compiled, $true)
        [IO.File]::WriteAllText($toolMarker, "$toolHash`n")
    }
    else {
        'Using compiled Source model.'
    }

    $parent = [IO.Path]::GetDirectoryName($Addon)
    [void][IO.Directory]::CreateDirectory($parent)
    $staged = Join-Path $parent ".garryscape-install.$([guid]::NewGuid())"
    try {
        foreach ($suffix in $suffixes) {
            if ([IO.File]::Exists("$model$suffix")) {
                Copy-Asset "$model$suffix" (Join-Path $staged "models/garryscape/$region$suffix")
            }
        }
        $materials = "materials/models/garryscape/$Name/regions"
        Copy-Asset (Join-Path $game "$materials/$id.vtf") (Join-Path $staged "$materials/$id.vtf")
        $vmts = @(Get-ChildItem -LiteralPath (Join-Path $game $materials) -Filter "${id}_*.vmt" -File)
        if (!$vmts.Count) { throw "No VMTs were generated for region $id." }
        foreach ($vmt in $vmts) {
            Copy-Asset $vmt.FullName (Join-Path $staged "$materials/$($vmt.Name)")
        }
        Copy-Asset (Join-Path $PSScriptRoot 'LICENSE') (Join-Path $staged 'LICENSE')
        $luaFiles = @(
            'autorun/server/garryscape_preview.lua'
            'autorun/client/garryscape_utilities.lua'
            'autorun/client/garryscape_shader.lua'
            'garryscape/atlas_shader.lua'
        )
        foreach ($lua in $luaFiles) {
            Copy-Asset (Join-Path $PSScriptRoot "addons/garryscape/lua/$lua") (Join-Path $staged "lua/$lua")
        }
        Copy-Asset (Join-Path $build 'manifest.json') (Join-Path $staged 'data/garryscape/terrain_preview.json')
        if (Test-Path -LiteralPath $Addon) { Remove-Item -LiteralPath $Addon -Recurse -Force }
        [IO.Directory]::Move($staged, $Addon)
    }
    finally {
        if ([IO.Directory]::Exists($staged)) { Remove-Item -LiteralPath $staged -Recurse -Force }
    }
    "Installed compiled region and preview Lua in $Addon"
    'Sources, glTF and compile.log remain in the output directory.'
    'The atlas materials require EGSM on the game client. See .github/README.md.'
}
catch {
    [Console]::Error.WriteLine("ERROR: $($_.Exception.Message)")
    exit 1
}
