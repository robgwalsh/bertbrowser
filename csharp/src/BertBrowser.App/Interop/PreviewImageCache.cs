using System.Windows.Media;

namespace BertBrowser.App.Interop;

/// <summary>
/// A small LRU of preview images, keyed by path, size and modified time.
/// </summary>
/// <remarks>
/// <see cref="ShellThumbnails"/> deliberately has no cache of its own — a tile's thumbnail is held
/// by its row and dies with it, which is the right lifetime there. The preview pane's is not: a
/// large shell preview costs tens of milliseconds, and arrowing down a list of photographs and back
/// up would pay it again for every row. Fifty images is a couple of screens of browsing.
///
/// The modified time is in the key on purpose: a preview that survived the file being edited would
/// be a lie, and the pane refreshes on the same folder watcher everything else does.
/// </remarks>
public sealed class PreviewImageCache(int capacity = 50)
{
    private readonly object _gate = new();
    private readonly Dictionary<string, LinkedListNode<Entry>> _map = new(StringComparer.OrdinalIgnoreCase);
    private readonly LinkedList<Entry> _order = new();

    private readonly record struct Entry(string Key, ImageSource? Image);

    public static string KeyFor(string path, int size, DateTime modifiedUtc) =>
        $"{path}|{size}|{modifiedUtc.Ticks}";

    /// <summary>Returns the cached image, or runs <paramref name="factory"/> outside the lock so a
    /// stalled shell handler cannot block every other preview thread.</summary>
    public ImageSource? GetOrAdd(string key, Func<ImageSource?> factory)
    {
        lock (_gate)
        {
            if (_map.TryGetValue(key, out var hit))
            {
                _order.Remove(hit);
                _order.AddFirst(hit);
                return hit.Value.Image;
            }
        }

        var image = factory();

        lock (_gate)
        {
            // Another thread may have resolved the same key while we were outside the lock.
            if (_map.TryGetValue(key, out var existing))
            {
                _order.Remove(existing);
                _order.AddFirst(existing);
                return existing.Value.Image;
            }

            var node = new LinkedListNode<Entry>(new Entry(key, image));
            _order.AddFirst(node);
            _map[key] = node;
            if (_map.Count > capacity)
            {
                var lru = _order.Last!;
                _order.RemoveLast();
                _map.Remove(lru.Value.Key);
            }
            return image;
        }
    }
}
