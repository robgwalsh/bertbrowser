using System.Runtime.Versioning;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;

namespace BertBrowser.Core.Benchmarking;

/// <summary>The facts a machine key is made from. Nothing here changes between reboots.</summary>
/// <param name="RuntimeMajorMinor">
/// <c>10.0</c>, not <c>10.0.2</c>: a monthly runtime patch must not orphan every baseline on the
/// machine. The full version is recorded beside the key in <see cref="MachineInfo.Runtime"/>.
/// </param>
public sealed record MachineFacts(
    string Cpu,
    int LogicalCores,
    int RamGb,
    int OsBuild,
    string RuntimeMajorMinor);

/// <summary>
/// Names the machine a baseline belongs to, so a comparison is always against the same hardware.
/// </summary>
/// <remarks>
/// The key is readable on purpose — <c>ryzen-9-7950x-32c-64gb-w26200-net10.0-3f9a1c2b</c> says what
/// it is in a directory listing — with a short hash on the end so two machines whose slugs collide
/// still file separately. Only <see cref="Probe"/> touches the OS; <see cref="Compute"/> is pure, and
/// that is what the tests pin.
/// </remarks>
public static partial class MachineKey
{
    /// <summary>Long enough for "amd-ryzen-9-7950x-16-core" whole; a file name, not a sentence.</summary>
    private const int MaxSlugLength = 32;

    public static string Compute(MachineFacts facts)
    {
        var canonical = string.Join('|',
            facts.Cpu.Trim(), facts.LogicalCores, facts.RamGb, facts.OsBuild, facts.RuntimeMajorMinor);
        var hash = Convert.ToHexStringLower(SHA256.HashData(Encoding.UTF8.GetBytes(canonical)))[..8];

        return $"{Slug(facts.Cpu)}-{facts.LogicalCores}c-{facts.RamGb}gb-w{facts.OsBuild}-net{facts.RuntimeMajorMinor}-{hash}";
    }

    /// <summary>
    /// A CPU name reduced to something that can be a file name: lowercase, vendor noise dropped,
    /// runs of anything that is not a letter or digit collapsed to one dash.
    /// </summary>
    public static string Slug(string cpu)
    {
        var text = cpu.ToLowerInvariant();
        text = VendorNoise().Replace(text, " ");
        text = ClockSuffix().Replace(text, " ");
        text = NonAlphanumeric().Replace(text, "-").Trim('-');

        if (text.Length > MaxSlugLength) text = text[..MaxSlugLength].TrimEnd('-');

        return text.Length == 0 ? "cpu" : text;
    }

    /// <summary>Reads the facts off the running machine.</summary>
    [SupportedOSPlatform("windows")]
    public static MachineFacts Probe()
    {
        var cpu = ReadCpuName() ?? "unknown-cpu";
        var ramGb = (int)Math.Round(GC.GetGCMemoryInfo().TotalAvailableMemoryBytes / (1024.0 * 1024 * 1024));
        var runtime = Environment.Version;

        return new MachineFacts(
            cpu,
            Environment.ProcessorCount,
            ramGb,
            Environment.OSVersion.Version.Build,
            $"{runtime.Major}.{runtime.Minor}");
    }

    [SupportedOSPlatform("windows")]
    private static string? ReadCpuName()
    {
        try
        {
            using var key = Microsoft.Win32.Registry.LocalMachine.OpenSubKey(
                @"HARDWARE\DESCRIPTION\System\CentralProcessor\0");
            // The registry value is padded with trailing spaces on some machines.
            return (key?.GetValue("ProcessorNameString") as string)?.Trim();
        }
        catch (Exception e) when (e is System.Security.SecurityException or IOException
                                      or UnauthorizedAccessException)
        {
            return null;
        }
    }

    [GeneratedRegex(@"\((r|tm|c)\)|\b(processor|cpu|with radeon graphics)\b", RegexOptions.IgnoreCase)]
    private static partial Regex VendorNoise();

    [GeneratedRegex(@"@\s*[\d.]+\s*ghz", RegexOptions.IgnoreCase)]
    private static partial Regex ClockSuffix();

    [GeneratedRegex(@"[^a-z0-9]+")]
    private static partial Regex NonAlphanumeric();
}
