$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$publicDir = Join-Path $root "public"
$iconDir = Join-Path $root "src-tauri\icons"
$candidates = @("logo.ico", "logo.png", "logo.jpg", "logo.jpeg", "logo.bmp", "logo.svg", "logo.webp")
$logo = $null

foreach ($name in $candidates) {
    $path = Join-Path $root $name
    if (Test-Path -LiteralPath $path) {
        $logo = Get-Item -LiteralPath $path
        break
    }
}

if ($null -eq $logo) {
    exit 0
}

New-Item -ItemType Directory -Force -Path $publicDir | Out-Null
New-Item -ItemType Directory -Force -Path $iconDir | Out-Null
Copy-Item -LiteralPath $logo.FullName -Destination (Join-Path $publicDir $logo.Name) -Force

function Write-IcoFromPngBytes {
    param(
        [byte[]] $PngBytes,
        [string] $OutputPath
    )

    $width = $PngBytes[16] * 16777216 + $PngBytes[17] * 65536 + $PngBytes[18] * 256 + $PngBytes[19]
    $height = $PngBytes[20] * 16777216 + $PngBytes[21] * 65536 + $PngBytes[22] * 256 + $PngBytes[23]
    $widthByte = if ($width -ge 256) { 0 } else { [byte]$width }
    $heightByte = if ($height -ge 256) { 0 } else { [byte]$height }
    $sizeBytes = [BitConverter]::GetBytes([UInt32]$PngBytes.Length)
    $offsetBytes = [BitConverter]::GetBytes([UInt32]22)

    $header = New-Object byte[] 22
    $header[2] = 1
    $header[4] = 1
    $header[6] = $widthByte
    $header[7] = $heightByte
    $header[10] = 1
    $header[12] = 32
    [Array]::Copy($sizeBytes, 0, $header, 14, 4)
    [Array]::Copy($offsetBytes, 0, $header, 18, 4)

    $output = New-Object byte[] ($header.Length + $PngBytes.Length)
    [Array]::Copy($header, 0, $output, 0, $header.Length)
    [Array]::Copy($PngBytes, 0, $output, $header.Length, $PngBytes.Length)
    [IO.File]::WriteAllBytes($OutputPath, $output)
}

$targetIco = Join-Path $iconDir "icon.ico"
switch ($logo.Extension.ToLowerInvariant()) {
    ".ico" {
        Copy-Item -LiteralPath $logo.FullName -Destination $targetIco -Force
    }
    ".png" {
        Write-IcoFromPngBytes -PngBytes ([IO.File]::ReadAllBytes($logo.FullName)) -OutputPath $targetIco
    }
    ".jpg" {
        Add-Type -AssemblyName System.Drawing
        $image = [Drawing.Image]::FromFile($logo.FullName)
        try {
            $pngPath = Join-Path $env:TEMP "coral-launcher-logo.png"
            $image.Save($pngPath, [Drawing.Imaging.ImageFormat]::Png)
            Copy-Item -LiteralPath $pngPath -Destination (Join-Path $publicDir "logo.png") -Force
            Write-IcoFromPngBytes -PngBytes ([IO.File]::ReadAllBytes($pngPath)) -OutputPath $targetIco
        } finally {
            $image.Dispose()
        }
    }
    ".jpeg" {
        Add-Type -AssemblyName System.Drawing
        $image = [Drawing.Image]::FromFile($logo.FullName)
        try {
            $pngPath = Join-Path $env:TEMP "coral-launcher-logo.png"
            $image.Save($pngPath, [Drawing.Imaging.ImageFormat]::Png)
            Copy-Item -LiteralPath $pngPath -Destination (Join-Path $publicDir "logo.png") -Force
            Write-IcoFromPngBytes -PngBytes ([IO.File]::ReadAllBytes($pngPath)) -OutputPath $targetIco
        } finally {
            $image.Dispose()
        }
    }
    ".bmp" {
        Add-Type -AssemblyName System.Drawing
        $image = [Drawing.Image]::FromFile($logo.FullName)
        try {
            $pngPath = Join-Path $env:TEMP "coral-launcher-logo.png"
            $image.Save($pngPath, [Drawing.Imaging.ImageFormat]::Png)
            Copy-Item -LiteralPath $pngPath -Destination (Join-Path $publicDir "logo.png") -Force
            Write-IcoFromPngBytes -PngBytes ([IO.File]::ReadAllBytes($pngPath)) -OutputPath $targetIco
        } finally {
            $image.Dispose()
        }
    }
}

Write-Host "Prepared logo: $($logo.FullName)"
