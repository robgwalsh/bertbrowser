namespace BertBrowser.Core.Services.Archives;

/// <summary>
/// Containers this repository cannot produce, kept as base64 and written to a temp file by the
/// tests that need them.
/// </summary>
/// <remarks>
/// <para>
/// <b>Base64 in a C# file rather than a checked-in binary</b>, and for the reason
/// <c>ThemeCatalog</c> keeps the built-in palettes as data: every shipped byte stays visible to a
/// reviewer, and the comment naming the exact command that produced it travels with it. A blob in
/// the repository is neither reviewable nor self-documenting.
/// </para>
/// <para>
/// <b>These are the ones that matter most</b>, because the rest of the archive fixtures are written
/// with SharpCompress itself — which makes those round-trip tests rather than compatibility tests.
/// Nothing in the dependency graph can write an encrypted zip or a header-encrypted 7z, so these
/// are the only evidence that the reader handles what other tools really produce.
/// </para>
/// </remarks>
internal static class ArchiveFixtures
{
    /// <summary>The password for <see cref="EncryptedZip"/>.</summary>
    public const string ZipPassword = "hunter2";

    /// <summary>The password for <see cref="HeaderEncryptedSevenZip"/>.</summary>
    public const string SevenZipPassword = "correct-horse";

    /// <summary>
    /// A zip holding <c>secret.txt</c> and <c>notes.txt</c>, AES-256 encrypted, headers in the
    /// clear — so the listing is readable and only the contents are not.
    /// </summary>
    /// <remarks>
    /// 7-Zip 25.x:
    /// <c>7z a -tzip -phunter2 -mem=AES256 enc.zip secret.txt notes.txt</c>
    /// </remarks>
    public const string EncryptedZip =
        "UEsDBDMAAQBjAJqLHl0AAAAAJwAAAAsAAAAJAAsAbm90ZXMudHh0AZkHAAIAQUUDAAACnFlaLcnYjNP9jyKR7Egyrm3r0LG/O0vm" +
        "GpP9Q5cBsogFdahIyERQSwMEMwABAGMAmoseXQAAAAAmAAAACgAAAAoACwBzZWNyZXQudHh0AZkHAAIAQUUDAAAnutkTAWK+mMVR" +
        "TnY1AFxpNwOuPQhd4u5/PjhFXoGcjIliHMmMslBLAQI/ADMAAQBjAJqLHl0AAAAAJwAAAAsAAAAJAC8AAAAAAAAAIAAAAAAAAABu" +
        "b3Rlcy50eHQKACAAAAAAAAEAGAA2CvWMxjjdATYK9YzGON0BNgr1jMY43QEBmQcAAgBBRQMAAFBLAQI/ADMAAQBjAJqLHl0AAAAA" +
        "JgAAAAoAAAAKAC8AAAAAAAAAIAAAAFkAAABzZWNyZXQudHh0CgAgAAAAAAABABgAAaj0jMY43QEBqPSMxjjdAbqA9IzGON0BAZkH" +
        "AAIAQUUDAABQSwUGAAAAAAIAAgDNAAAAsgAAAAAA";

    /// <summary>
    /// A 7z with its <em>headers</em> encrypted, so not even the names can be listed without the
    /// password — the case that has to look different from an encrypted-contents zip.
    /// </summary>
    /// <remarks>
    /// 7-Zip 25.x:
    /// <c>7z a -t7z -pcorrect-horse -mhe=on enchdr.7z secret.txt notes.txt</c>
    /// </remarks>
    public const string HeaderEncryptedSevenZip =
        "N3q8ryccAASKxhnysAAAAAAAAAA+AAAAAAAAANeaSMmORgcj9UHYJ6oJ0HuFjuISYs7nVh6Ea8Y/+KOZO0KfX8eLFfkVanwLMxqJ" +
        "OIkRR0FrSFFLWtYotGnjDpa51pm1jFAT9mAVEIRVwnDEqmHIXeZJvvcLUVrZqfeS2YRkOl8/bx5tv4RxQ+Ot4bW/26AO+h7SRP1n" +
        "tLLzZV08AUPGOhnQDxxWeMe7+I3z0z9K4ymfKftaHws8L8sUsL993FMREyDWXy6GfuWzbo00uplhhBcGIAEJgJAABwsBAAIkBvEH" +
        "ARJTD7duEwaZGJFAP0kVEQmi5r0jAwEBBV0AEAAAAQAMgISAngoBo37mIAAA";

    /// <summary>
    /// An ordinary 7z holding the same two files. Nothing in the graph can <em>write</em> 7z, so
    /// without this there would be no evidence the reader handles the format at all.
    /// </summary>
    /// <remarks>
    /// 7-Zip 25.x: <c>7z a -t7z plain.7z secret.txt notes.txt</c>
    /// </remarks>
    public const string PlainSevenZip =
        "N3q8ryccAAQm7sIAgQAAAAAAAAAgAAAAAAAAAJBsR/4BABRhbHNvIHNlY3JldGNsYXNzaWZpZWQAAACBMweuD8+SbmAP6+qcvzY9" +
        "/nHKUfIBttljND19I61lOccsXwBFigB+kaExKBg6qKVdGIsXhoIiXqndBZ1CuVXh1KU+KEr3BbWiHV/exhdmDVOsDDRJvuEzQZid" +
        "SjEFX/mu6jMJvyAXBhkBCWgABwsBAAEjAwEBBV0AEAAADH4KAXdY2lkAAA==";

    /// <summary>Writes one of the fixtures to <paramref name="path"/> and returns it.</summary>
    public static string WriteTo(string path, string base64)
    {
        File.WriteAllBytes(path, Convert.FromBase64String(base64));
        return path;
    }
}
