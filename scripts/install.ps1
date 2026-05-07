# Install script for neoc (Windows).
# Usage: irm https://raw.githubusercontent.com/flying-dice/neoc/main/scripts/install.ps1 | iex
#
# Environment variables:
#   NEOC_INSTALL_DIR  - where to install (default: $env:USERPROFILE\.neoc\bin)
#   NEOC_VERSION      - version tag to install (default: latest)

$ErrorActionPreference = "Stop"

$Repo = "flying-dice/neoc"
$Binary = "neoc.exe"

$InstallDir = if ($env:NEOC_INSTALL_DIR) {
    $env:NEOC_INSTALL_DIR
} else {
    Join-Path $env:USERPROFILE ".neoc\bin"
}

$Version = if ($env:NEOC_VERSION) {
    $env:NEOC_VERSION
} else {
    "latest"
}

function Resolve-Version {
    if ($Version -eq "latest") {
        $release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
        if (-not $release.tag_name) {
            Write-Error "Could not resolve latest version"
            exit 1
        }
        return $release.tag_name
    }
    return $Version
}

function Get-Target {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    switch ($arch) {
        "X64"   { return "x86_64-pc-windows-msvc" }
        "Arm64" { return "aarch64-pc-windows-msvc" }
        default {
            Write-Error "Unsupported architecture: $arch"
            exit 1
        }
    }
}

function Install-Neoc {
    Write-Host "Installing neoc..."

    $target = Get-Target
    $resolvedVersion = Resolve-Version
    $artifactName = "neoc-$target"

    Write-Host "  Platform: $target"
    Write-Host "  Version:  $resolvedVersion"
    Write-Host "  Install:  $InstallDir"

    $downloadUrl = "https://github.com/$Repo/releases/download/$resolvedVersion/$artifactName.zip"

    $tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
    New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

    try {
        $archivePath = Join-Path $tmpDir "archive.zip"
        Write-Host "  Downloading $downloadUrl ..."

        try {
            Invoke-WebRequest -Uri $downloadUrl -OutFile $archivePath -UseBasicParsing
        } catch {
            Write-Error "Download failed - check that version '$resolvedVersion' exists and has a release asset for '$target'"
            exit 1
        }

        Expand-Archive -Path $archivePath -DestinationPath $tmpDir -Force

        if (-not (Test-Path $InstallDir)) {
            New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
        }

        $src = Join-Path $tmpDir $Binary
        $dest = Join-Path $InstallDir $Binary
        Move-Item -Path $src -Destination $dest -Force

        Write-Host ""
        Write-Host "neoc $resolvedVersion installed to $dest"

        # Check if install dir is on PATH
        $currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
        if ($currentPath -notlike "*$InstallDir*") {
            Write-Host ""
            Write-Host "Add to your PATH (current session):"
            Write-Host "  `$env:PATH = `"$InstallDir;`$env:PATH`""
            Write-Host ""
            Write-Host "Add permanently:"
            Write-Host "  [Environment]::SetEnvironmentVariable('PATH', `"$InstallDir;`$([Environment]::GetEnvironmentVariable('PATH', 'User'))`", 'User')"
        }
    } finally {
        Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Install-Neoc
