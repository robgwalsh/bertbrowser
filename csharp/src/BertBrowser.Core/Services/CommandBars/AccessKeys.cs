using System.Text;

namespace BertBrowser.Core.Services.CommandBars;

/// <summary>
/// Access keys in captions. A caption marks its key the way Windows always has — <c>&amp;File</c>,
/// with <c>&amp;&amp;</c> for a real ampersand — and anything that comes from data (a workspace's
/// name, a command's own name) is <see cref="Escape"/>d before it gets here, so "R&amp;D" does not
/// become a menu answering to D.
/// </summary>
public static class AccessKeys
{
    /// <summary>Text that is not a caption, made safe to use as one.</summary>
    public static string Escape(string text) => text.Replace("&", "&&");

    /// <summary>The caption as plain words.</summary>
    public static string Strip(string caption)
    {
        var plain = new StringBuilder(caption.Length);
        for (var i = 0; i < caption.Length; i++)
        {
            if (caption[i] == '&' && i + 1 < caption.Length) i++;
            plain.Append(caption[i]);
        }
        return plain.ToString();
    }

    /// <summary>The key a caption answers to, uppercased, or null when it marks none.</summary>
    public static char? KeyOf(string caption)
    {
        for (var i = 0; i + 1 < caption.Length; i++)
        {
            if (caption[i] != '&') continue;
            if (caption[i + 1] != '&') return char.ToUpperInvariant(caption[i + 1]);
            i++;
        }
        return null;
    }

    /// <summary>
    /// Gives every caption of one menu a key of its own where it can. A key a caption already
    /// marks is kept — and wins over any chosen here — then each remaining caption takes the first
    /// free letter among its word initials, then among all its letters. One that finds none goes
    /// without rather than share.
    /// </summary>
    public static IReadOnlyList<string> Assign(IReadOnlyList<string> captions)
    {
        var taken = new HashSet<char>();
        foreach (var caption in captions)
        {
            if (KeyOf(caption) is { } key) taken.Add(key);
        }

        var result = new string[captions.Count];
        for (var i = 0; i < captions.Count; i++)
        {
            var caption = captions[i];
            result[i] = KeyOf(caption) is null && Choose(caption, taken) is { } at
                ? caption.Insert(at, "&")
                : caption;
        }
        return result;
    }

    /// <summary>Where to put the mark, claiming the key; null when every candidate is taken.</summary>
    private static int? Choose(string caption, HashSet<char> taken)
    {
        // Word initials first: "Date modified" would rather answer to D or M than to A.
        foreach (var initialsOnly in new[] { true, false })
        {
            for (var i = 0; i < caption.Length; i++)
            {
                var c = caption[i];
                if (c == '&') { i++; continue; }
                if (!char.IsLetterOrDigit(c)) continue;
                if (initialsOnly && i > 0 && char.IsLetterOrDigit(caption[i - 1])) continue;
                if (taken.Add(char.ToUpperInvariant(c))) return i;
            }
        }
        return null;
    }

    /// <summary>
    /// The caption in the spelling WPF reads: an underscore before the key, a doubled underscore
    /// for a real one. Kept here, beside the rule it translates, so it can be tested without a
    /// window.
    /// </summary>
    public static string ToUnderscored(string caption)
    {
        var text = new StringBuilder(caption.Length + 2);
        for (var i = 0; i < caption.Length; i++)
        {
            var c = caption[i];
            if (c == '_') text.Append("__");
            else if (c == '&' && i + 1 < caption.Length)
            {
                i++;
                text.Append(caption[i] == '&' ? "&" : "_" + (caption[i] == '_' ? "_" : caption[i].ToString()));
            }
            else text.Append(c);
        }
        return text.ToString();
    }
}
