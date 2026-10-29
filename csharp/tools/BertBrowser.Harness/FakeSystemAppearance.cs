using BertBrowser.App.Theming;
using BertBrowser.Core.Theming;

namespace BertBrowser.Harness;

/// <summary>
/// A posed Windows appearance.
/// </summary>
/// <remarks>
/// A scripted run must not read the developer's own light/dark or high-contrast setting, or every
/// theming screenshot would depend on how the machine that took it happened to be configured — a
/// developer with high contrast turned on gets a different <c>themes.bbs</c> than everyone else.
/// Starts dark, which is what the app's own default has always been.
/// </remarks>
internal sealed class FakeSystemAppearance : ISystemAppearance
{
    private SystemAppearance _current = SystemAppearance.Dark;

    public SystemAppearance Current => _current;

    public event EventHandler? Changed;

    public void Set(SystemAppearance value)
    {
        if (value == _current) return;

        _current = value;
        Changed?.Invoke(this, EventArgs.Empty);
    }

    public void Dispose()
    {
    }
}
