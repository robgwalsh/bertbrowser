using System.Globalization;
using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Markup;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using ShapePath = System.Windows.Shapes.Path;

namespace BertBrowser.IconSheet;

/// <summary>
/// Renders every icon in <c>Resources/Icons.xaml</c> to a labelled PNG, so an icon is checked by
/// looking at it.
/// </summary>
/// <remarks>
/// <para>
/// A wrong icon is the one UI mistake nothing else catches: it compiles, it renders, it is simply
/// the wrong picture, and no test and no code review sees it. Four shipped that way while the app
/// drew icons as raw Segoe font codepoints — a smiley face on the split-pane button, a mouse cursor
/// on "Open in new pane", a down arrow on "Delete permanently". Names made those unrepeatable at the
/// call site; this makes the one remaining place they could enter — the manifest — reviewable too.
/// </para>
/// <para>
/// It renders whatever is really in the dictionary, not a copy of it, so a geometry that failed to
/// convert shows up as a blank cell rather than as a passing build. Nothing shows a window: the grid
/// is measured, arranged and rendered through <see cref="RenderTargetBitmap"/>, the same way the UI
/// harness photographs a dialog, so running this never lands anything on the screen of whoever is
/// using the machine.
/// </para>
/// <para><c>dotnet run --project tools/icon/IconSheet -- [Icons.xaml] [out.png]</c></para>
/// </remarks>
internal static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        var source = Path.GetFullPath(args.Length > 0
            ? args[0]
            : Path.Combine(AppContext.BaseDirectory, "..", "..", "..", "..", "..", "..",
                "src", "BertBrowser.App", "Resources", "Icons.xaml"));

        if (!File.Exists(source))
        {
            Console.Error.WriteLine($"no icon dictionary at {source}");
            return 2;
        }

        _ = new Application();

        using var stream = File.OpenRead(source);
        if (XamlReader.Load(stream) is not ResourceDictionary dictionary)
        {
            Console.Error.WriteLine($"{source} is not a ResourceDictionary.");
            return 2;
        }

        var icons = dictionary.Keys
            .Cast<object>()
            .Select(key => (Key: key.ToString() ?? "", Geometry: dictionary[key] as Geometry))
            .Where(entry => entry.Geometry is not null)
            .OrderBy(entry => entry.Key, StringComparer.Ordinal)
            .ToList();

        if (icons.Count == 0)
        {
            Console.Error.WriteLine($"{source} holds no geometries.");
            return 2;
        }

        var path = Path.GetFullPath(args.Length > 1 ? args[1] : "icons.png");
        Render(icons, path);
        Console.WriteLine($"{icons.Count} icons -> {path}");
        return 0;
    }

    private static void Render(List<(string Key, Geometry? Geometry)> icons, string path)
    {
        const int cell = 118;
        const int cols = 8;
        var rows = (int)Math.Ceiling(icons.Count / (double)cols);

        // White, not the theme: this is for judging a shape, and every icon is a monochrome outline
        // meant to take its colour from whatever it sits on.
        var grid = new UniformGrid { Columns = cols, Rows = rows, Background = Brushes.White };
        foreach (var (key, geometry) in icons)
        {
            var box = new StackPanel { Width = cell, Height = cell };
            box.Children.Add(new ShapePath
            {
                Data = geometry,
                Fill = Brushes.Black,
                Stretch = Stretch.Uniform,
                Width = 64,
                Height = 64,
                HorizontalAlignment = HorizontalAlignment.Center,
                Margin = new Thickness(0, 10, 0, 6),
            });

            // The name under every icon is the whole point — a sheet you cannot read names off is a
            // picture, not an answer.
            box.Children.Add(new TextBlock
            {
                Text = key.StartsWith("Icon.", StringComparison.Ordinal) ? key[5..] : key,
                FontFamily = new FontFamily("Consolas"),
                FontSize = 12,
                Foreground = Brushes.DimGray,
                TextAlignment = TextAlignment.Center,
                TextWrapping = TextWrapping.Wrap,
                HorizontalAlignment = HorizontalAlignment.Center,
            });
            grid.Children.Add(box);
        }

        var width = cols * cell;
        var height = rows * cell;
        grid.Measure(new Size(width, height));
        grid.Arrange(new Rect(0, 0, width, height));
        grid.UpdateLayout();

        var target = new RenderTargetBitmap(width, height, 96, 96, PixelFormats.Pbgra32);
        target.Render(grid);

        var encoder = new PngBitmapEncoder();
        encoder.Frames.Add(BitmapFrame.Create(target));
        using var file = File.Create(path);
        encoder.Save(file);
    }
}
