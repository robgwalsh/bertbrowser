<#
.SYNOPSIS
    Draws src/BertBrowser.App/Assets/app.ico from scratch.

.DESCRIPTION
    This script is the icon's source of truth -- there is no .svg or .psd behind it. Run it after
    changing anything below and commit the regenerated .ico:

        powershell -NoProfile -ExecutionPolicy Bypass -File tools/icon/build-app-icon.ps1

    Windows PowerShell 5.1 (`powershell.exe`), not `pwsh`: the drawing is GDI+ via System.Drawing,
    which is Windows-only and not loaded by default in PowerShell 7.

    The mark is a folder whose face is the app's own layout -- sidebar tree, a splitter, two panes
    of name/size rows, one selected row in the accent colour. Palette is the app's default theme
    (VS Code Dark+) over a folder amber.

    THREE DRAWINGS, NOT ONE SCALED. A 256px drawing shrunk to 16px is mush: at 16px one device
    pixel is 16 units of the 256-unit design space, so every row, divider and rim narrower than
    that simply dissolves into grey. So there are three tiers, each laid out on a grid that lands
    on whole pixels at its own base size:

        S  16, 20, 24    grid 16   flat sidebar strip, one splitter, 3 rows a pane
        M  32, 40, 48    grid 8    sidebar tree, both dividers, 4 rows, no size column
        L  64, 96, 128,  grid 1    everything, including the right-hand size column
           256

    Sizes off a tier's base (20, 24, 40, 96...) scale that tier's art, which is why each tier's
    detail is sized for the *smallest* member: legible there is legible across the rest.

    Frames <=48px are stored as BMP/DIB and >=64px as PNG. That is the conventional layout and the
    one every shell surface reads; a PNG-only icon is legal since Vista but fewer tools handle it,
    installer authoring among them -- and this file is what release.yml hands to `vpk --icon`.
#>

[CmdletBinding()]
param(
    [string]$OutputPath,
    [string]$PreviewDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Resolved here rather than as a parameter default: $PSScriptRoot is not populated in the param
# block when the script is invoked as `powershell -File <relative path>`, which is how it is meant
# to be run.
if (-not $OutputPath) {
    $OutputPath = Join-Path $PSScriptRoot '..\..\src\BertBrowser.App\Assets\app.ico'
}

Add-Type -AssemblyName System.Drawing

# ---------------------------------------------------------------------------------------------
# Palette
# ---------------------------------------------------------------------------------------------

function C([string]$hex) { [System.Drawing.ColorTranslator]::FromHtml($hex) }

$Palette = @{
    AmberTop  = C '#EFC384'   # folder, top of the gradient
    AmberBot  = C '#C0862F'   # folder, bottom
    AmberEdge = C '#F7DCB4'   # highlight along the top of the tab
    FaceBg    = C '#232325'   # the window face inside the folder
    FaceRim   = C '#3C3C40'   # hairline separating the face from the amber
    SideBg    = C '#2E2E32'   # sidebar column
    SideBgS   = C '#3A3A40'   # ...at tier S, where 2px of it has to read on its own
    Divider   = C '#191919'   # sidebar edge and the pane splitter
    NameBar   = C '#C3C9D1'   # a file name
    NameBarS  = C '#D4D9E0'   # ...at tier S
    SizeBar   = C '#6E757E'   # the size column -- dimmer, as it is in the app
    TreeBar   = C '#8A929C'   # a folder in the sidebar tree
    Accent    = C '#1F6FEB'   # the selected row
    OnAccent  = C '#FFFFFF'
    OnAccentD = C '#BBD3F7'
}

# ---------------------------------------------------------------------------------------------
# Geometry, in a 256-unit design space. See the tier note above.
# ---------------------------------------------------------------------------------------------

# Tier L -- 64, 96, 128, 256. Full detail: sidebar tree, both dividers, name + size columns.
$TierL = @{
    Tab         = @{ x =  16; y =  36; w =  92; h =  44; r = 10 }
    Body        = @{ x =  16; y =  60; w = 224; h = 164; r = 16 }
    Face        = @{ x =  40; y =  86; w = 176; h = 114; r = 10 }
    Rim         = 2
    Sidebar     = @{ x =  40; w =  32 }
    SideDivider = @{ x =  72; w =   3 }
    Splitter    = @{ x = 144; w =   4 }
    SideBg      = $Palette.SideBg
    NameColor   = $Palette.NameBar
    TreeRows    = @(
        @{ x = 46; y = 100; w = 20 }, @{ x = 50; y = 116; w = 16 },
        @{ x = 50; y = 132; w = 14 }, @{ x = 46; y = 148; w = 18 },
        @{ x = 50; y = 164; w = 16 }, @{ x = 46; y = 180; w = 12 }
    )
    TreeH       = 6
    RowY        = @(99, 119, 139, 159, 179)
    RowH        = 8
    RowR        = 4
    SelIndex    = 2
    PaneA       = @{ NameX =  83; NameW = @(35, 26, 31, 22, 29); SizeX = 124; SizeW = 12; SelX = 75; SelW = 69; SelPad = 6; SelR = 2 }
    PaneB       = @{ NameX = 156; NameW = @(34, 28, 24, 30, 26); SizeX = 196; SizeW = 12 }
}

# Tier M -- 32, 40, 48. Everything on an 8-unit grid, so at 32px every edge is a whole pixel.
# The size column is gone: at 32px it would be a 2px smudge beside a 6px name bar.
$TierM = @{
    Tab         = @{ x =  16; y =  40; w =  88; h =  40; r =  8 }
    Body        = @{ x =  16; y =  64; w = 224; h = 160; r = 16 }
    Face        = @{ x =  40; y =  88; w = 176; h = 112; r =  8 }
    Rim         = 0
    Sidebar     = @{ x =  40; w =  24 }
    SideDivider = @{ x =  64; w =   8 }
    Splitter    = @{ x = 144; w =   8 }
    SideBg      = $Palette.SideBg
    NameColor   = $Palette.NameBar
    TreeRows    = @(
        @{ x = 48; y = 104; w =  8 }, @{ x = 48; y = 128; w =  8 },
        @{ x = 48; y = 152; w =  8 }, @{ x = 48; y = 176; w =  8 }
    )
    TreeH       = 8
    RowY        = @(104, 128, 152, 176)
    RowH        = 8
    RowR        = 4
    SelIndex    = 2
    PaneA       = @{ NameX =  80; NameW = @(48, 32, 40, 32); SizeX = 0; SizeW = 0; SelX = 72; SelW = 72; SelPad = 8; SelR = 0 }
    PaneB       = @{ NameX = 160; NameW = @(40, 32, 40, 32); SizeX = 0; SizeW = 0 }
}

# Tier S -- 16, 20, 24. A 16-unit grid: one unit of slack here is a whole pixel at 16px.
#
# The sidebar is gone entirely, and the face is pulled in to a 2px amber frame. Both are the same
# call: at 16px the folder has to be recognisable *first*, from across a taskbar, and a face that
# fills the folder leaves the silhouette as a thin orange outline around a dark blob. What is left
# inside -- a splitter and three rows across two panes -- is the most this size can carry. Rows are
# square, since a 1px bar has nowhere to put a radius.
$TierS = @{
    Tab         = @{ x =  16; y =  32; w =  80; h =  48; r =  8 }
    Body        = @{ x =  16; y =  64; w = 224; h = 160; r = 16 }
    Face        = @{ x =  48; y =  96; w = 160; h = 112; r = 16 }
    Rim         = 0
    Sidebar     = $null
    SideDivider = $null
    Splitter    = @{ x = 128; w =  16 }
    SideBg      = $Palette.SideBgS
    NameColor   = $Palette.NameBarS
    TreeRows    = @()
    TreeH       = 0
    RowY        = @(112, 144, 176)
    RowH        = 16
    RowR        = 0
    SelIndex    = 1
    PaneA       = @{ NameX =  64; NameW = @(48, 48, 32); SizeX = 0; SizeW = 0; SelX = 48; SelW = 80; SelPad = 0; SelR = 0 }
    PaneB       = @{ NameX = 160; NameW = @(32, 32, 32); SizeX = 0; SizeW = 0 }
}

function Get-Tier([int]$size) {
    if ($size -le 24) { return $TierS }
    if ($size -le 48) { return $TierM }
    return $TierL
}

# ---------------------------------------------------------------------------------------------
# Drawing
# ---------------------------------------------------------------------------------------------

function New-RoundRect([single]$x, [single]$y, [single]$w, [single]$h, [single]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    if ($r -le 0.05) {
        $p.AddRectangle((New-Object System.Drawing.RectangleF $x, $y, $w, $h))
    } else {
        $d = [single]($r * 2)
        $p.AddArc($x,           $y,           $d, $d, 180, 90)
        $p.AddArc($x + $w - $d, $y,           $d, $d, 270, 90)
        $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d,   0, 90)
        $p.AddArc($x,           $y + $h - $d, $d, $d,  90, 90)
        $p.CloseFigure()
    }
    $p
}

# The folder tab: rounded across the top, square along the bottom, where the body swallows it.
function New-TabPath([single]$x, [single]$y, [single]$w, [single]$h, [single]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = [single]($r * 2)
    $p.AddArc($x,           $y, $d, $d, 180, 90)
    $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $p.AddLine([single]($x + $w), [single]($y + $h), [single]$x, [single]($y + $h))
    $p.CloseFigure()
    $p
}

function New-IconBitmap([int]$size) {
    $t = Get-Tier $size
    $k = $size / 256.0
    # Everything below is written in design units; U turns one into device pixels.
    function U([double]$v) { [single]($v * $k) }

    $bmp = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $bmp.SetResolution(96, 96)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    try {
        $g.SmoothingMode      = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $g.PixelOffsetMode    = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
        $g.Clear([System.Drawing.Color]::Transparent)

        # --- the folder -------------------------------------------------------------------
        # Tab and body are filled separately with one gradient spanning both, so the seam where
        # they overlap is invisible: at any y the two fills resolve to the same colour.
        $folderTop = U $t.Tab.y
        $folderBot = U ($t.Body.y + $t.Body.h)
        $grad = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
            (New-Object System.Drawing.PointF ([single]0), $folderTop),
            (New-Object System.Drawing.PointF ([single]0), $folderBot),
            $Palette.AmberTop, $Palette.AmberBot)
        $tab  = New-TabPath   (U $t.Tab.x)  (U $t.Tab.y)  (U $t.Tab.w)  (U $t.Tab.h)  (U $t.Tab.r)
        $body = New-RoundRect (U $t.Body.x) (U $t.Body.y) (U $t.Body.w) (U $t.Body.h) (U $t.Body.r)
        $g.FillPath($grad, $tab)
        $g.FillPath($grad, $body)
        $tab.Dispose(); $body.Dispose(); $grad.Dispose()

        # A highlight along the top of the tab -- the one thing giving the folder any depth.
        if ($size -ge 48) {
            $hl = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(70, $Palette.AmberEdge))
            $hlPath = New-TabPath (U ($t.Tab.x + 3)) (U ($t.Tab.y + 2)) (U ($t.Tab.w - 6)) (U 4) (U $t.Tab.r)
            $g.FillPath($hl, $hlPath)
            $hl.Dispose(); $hlPath.Dispose()
        }

        # --- the window face --------------------------------------------------------------
        $face = New-RoundRect (U $t.Face.x) (U $t.Face.y) (U $t.Face.w) (U $t.Face.h) (U $t.Face.r)
        $faceBrush = New-Object System.Drawing.SolidBrush $Palette.FaceBg
        $g.FillPath($faceBrush, $face)
        $faceBrush.Dispose()

        # Clip to the face for everything inside it: the sidebar and panes then need no corner
        # handling of their own, and nothing can bleed onto the amber.
        $g.SetClip($face)

        $fy = $t.Face.y
        $fh = $t.Face.h

        if ($null -ne $t.Sidebar) {
            $sideBrush = New-Object System.Drawing.SolidBrush $t.SideBg
            $g.FillRectangle($sideBrush, (U $t.Sidebar.x), (U $fy), (U $t.Sidebar.w), (U $fh))
            $sideBrush.Dispose()
        }

        $divBrush = New-Object System.Drawing.SolidBrush $Palette.Divider
        if ($null -ne $t.SideDivider) {
            $g.FillRectangle($divBrush, (U $t.SideDivider.x), (U $fy), (U $t.SideDivider.w), (U $fh))
        }
        $g.FillRectangle($divBrush, (U $t.Splitter.x), (U $fy), (U $t.Splitter.w), (U $fh))
        $divBrush.Dispose()

        # Sidebar tree.
        if ($t.TreeRows.Count -gt 0) {
            $treeBrush = New-Object System.Drawing.SolidBrush $Palette.TreeBar
            foreach ($row in $t.TreeRows) {
                $p = New-RoundRect (U $row.x) (U $row.y) (U $row.w) (U $t.TreeH) (U ($t.TreeH / 2))
                $g.FillPath($treeBrush, $p); $p.Dispose()
            }
            $treeBrush.Dispose()
        }

        # --- pane rows --------------------------------------------------------------------
        $nameBrush   = New-Object System.Drawing.SolidBrush $t.NameColor
        $sizeBrush   = New-Object System.Drawing.SolidBrush $Palette.SizeBar
        $accentBrush = New-Object System.Drawing.SolidBrush $Palette.Accent
        $onAccent    = New-Object System.Drawing.SolidBrush $Palette.OnAccent
        $onAccentDim = New-Object System.Drawing.SolidBrush $Palette.OnAccentD

        # The selected row goes down first: the accent bar spans pane A edge to edge, and that
        # row's name and size bars then sit on top of it in their on-accent colours.
        $a = $t.PaneA
        $b = $t.PaneB
        $selY = $t.RowY[$t.SelIndex]
        $selPath = New-RoundRect (U $a.SelX) (U ($selY - $a.SelPad)) (U $a.SelW) (U ($t.RowH + 2 * $a.SelPad)) (U $a.SelR)
        $g.FillPath($accentBrush, $selPath); $selPath.Dispose()

        for ($i = 0; $i -lt $t.RowY.Count; $i++) {
            $y = $t.RowY[$i]
            $selected = ($i -eq $t.SelIndex)

            $nb = if ($selected) { $onAccent }    else { $nameBrush }
            $sb = if ($selected) { $onAccentDim } else { $sizeBrush }

            $p = New-RoundRect (U $a.NameX) (U $y) (U $a.NameW[$i]) (U $t.RowH) (U $t.RowR)
            $g.FillPath($nb, $p); $p.Dispose()
            if ($a.SizeW -gt 0) {
                $p = New-RoundRect (U $a.SizeX) (U $y) (U $a.SizeW) (U $t.RowH) (U $t.RowR)
                $g.FillPath($sb, $p); $p.Dispose()
            }

            $p = New-RoundRect (U $b.NameX) (U $y) (U $b.NameW[$i]) (U $t.RowH) (U $t.RowR)
            $g.FillPath($nameBrush, $p); $p.Dispose()
            if ($b.SizeW -gt 0) {
                $p = New-RoundRect (U $b.SizeX) (U $y) (U $b.SizeW) (U $t.RowH) (U $t.RowR)
                $g.FillPath($sizeBrush, $p); $p.Dispose()
            }
        }

        $nameBrush.Dispose(); $sizeBrush.Dispose(); $accentBrush.Dispose()
        $onAccent.Dispose(); $onAccentDim.Dispose()

        $g.ResetClip()

        # A hairline between the face and the amber, so the dark rectangle reads as inset rather
        # than as a hole. Only at sizes with a pixel to spare for it.
        if ($t.Rim -gt 0) {
            $pen = New-Object System.Drawing.Pen($Palette.FaceRim, (U $t.Rim))
            $pen.Alignment = [System.Drawing.Drawing2D.PenAlignment]::Inset
            $g.DrawPath($pen, $face)
            $pen.Dispose()
        }
        $face.Dispose()
    } finally {
        $g.Dispose()
    }
    $bmp
}

# ---------------------------------------------------------------------------------------------
# ICO assembly
# ---------------------------------------------------------------------------------------------

function ConvertTo-Png([System.Drawing.Bitmap]$bmp) {
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    # Unary comma: without it PowerShell unrolls the byte[] into an Object[] on the way out,
    # and BinaryWriter.Write then takes an overload that is not Write(byte[]).
    , $ms.ToArray()
}

# A 32bpp bottom-up DIB plus the all-zero AND mask an ICO directory entry expects. biHeight is
# doubled because the header describes the colour rows and the mask rows together.
function ConvertTo-Dib([System.Drawing.Bitmap]$bmp) {
    $w = $bmp.Width; $h = $bmp.Height
    $rect = New-Object System.Drawing.Rectangle 0, 0, $w, $h
    $data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly,
                          [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try {
        $stride = $data.Stride
        $buf = New-Object byte[] ($stride * $h)
        [System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $buf, 0, $buf.Length)
    } finally {
        $bmp.UnlockBits($data)
    }

    $ms = New-Object System.IO.MemoryStream
    $bw = New-Object System.IO.BinaryWriter($ms)
    $bw.Write([uint32]40)          # biSize
    $bw.Write([int32]$w)           # biWidth
    $bw.Write([int32]($h * 2))     # biHeight -- colour rows + mask rows
    $bw.Write([uint16]1)           # biPlanes
    $bw.Write([uint16]32)          # biBitCount
    $bw.Write([uint32]0)           # biCompression = BI_RGB
    $bw.Write([uint32]($w * 4 * $h))
    $bw.Write([int32]0); $bw.Write([int32]0)
    $bw.Write([uint32]0); $bw.Write([uint32]0)

    for ($y = $h - 1; $y -ge 0; $y--) { $bw.Write($buf, $y * $stride, $w * 4) }

    $maskStride = [int][math]::Floor(($w + 31) / 32) * 4
    $bw.Write((New-Object byte[] ($maskStride * $h)))
    $bw.Flush()
    , $ms.ToArray()
}

$sizes = @(16, 20, 24, 32, 40, 48, 64, 96, 128, 256)

$frames = foreach ($size in $sizes) {
    $bmp = New-IconBitmap $size
    try {
        if ($PreviewDir) {
            if (-not (Test-Path $PreviewDir)) { New-Item -ItemType Directory -Path $PreviewDir | Out-Null }
            $bmp.Save((Join-Path $PreviewDir "app-$size.png"), [System.Drawing.Imaging.ImageFormat]::Png)
        }
        $png = $size -ge 64
        [pscustomobject]@{
            Size  = $size
            IsPng = $png
            Bytes = if ($png) { ConvertTo-Png $bmp } else { ConvertTo-Dib $bmp }
        }
    } finally {
        $bmp.Dispose()
    }
}

$ms = New-Object System.IO.MemoryStream
$bw = New-Object System.IO.BinaryWriter($ms)
$bw.Write([uint16]0)                    # reserved
$bw.Write([uint16]1)                    # type: icon
$bw.Write([uint16]$frames.Count)

$offset = 6 + 16 * $frames.Count
foreach ($f in $frames) {
    $dim = if ($f.Size -ge 256) { 0 } else { $f.Size }
    $bw.Write([byte]$dim)               # width  (0 means 256)
    $bw.Write([byte]$dim)               # height
    $bw.Write([byte]0)                  # colours in palette
    $bw.Write([byte]0)                  # reserved
    $bw.Write([uint16]1)                # planes
    $bw.Write([uint16]32)               # bits per pixel
    $bw.Write([uint32]$f.Bytes.Length)
    $bw.Write([uint32]$offset)
    $offset += $f.Bytes.Length
}
foreach ($f in $frames) { $bw.Write($f.Bytes) }
$bw.Flush()

$OutputPath = [System.IO.Path]::GetFullPath($OutputPath)
[System.IO.File]::WriteAllBytes($OutputPath, $ms.ToArray())
$bw.Dispose()

$kb = [math]::Round((Get-Item $OutputPath).Length / 1KB, 1)
Write-Host "Wrote $OutputPath -- $($frames.Count) frames, $kb KB"
foreach ($f in $frames) {
    Write-Host ("  {0,3}x{1,-3} {2,-4} {3,6} bytes" -f $f.Size, $f.Size, $(if ($f.IsPng) { 'PNG' } else { 'BMP' }), $f.Bytes.Length)
}
