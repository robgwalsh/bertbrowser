using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;

namespace BertBrowser.App.Views;

/// <summary>
/// Makes a seek bar follow the mouse for as long as the button is held, wherever it was pressed.
/// </summary>
/// <remarks>
/// A plain <see cref="Slider"/> only drags from its thumb. <c>IsMoveToPointEnabled</c> jumps the
/// thumb to a press on the track, but the thumb never takes the press — so the drag that follows
/// does nothing until the button is released and pressed again, which is not how any player's seek
/// bar behaves. This takes the press away from the slider altogether: it captures the mouse, and
/// every move sets <see cref="RangeBase.Value"/> from the point under the cursor
/// (<see cref="Track.ValueFromPoint"/>, so the thumb centres on it). Keyboard seeking is left alone.
/// </remarks>
public sealed class SliderScrubber
{
    private readonly Slider _slider;
    private Track? _track;

    public SliderScrubber(Slider slider)
    {
        _slider = slider;
        slider.PreviewMouseLeftButtonDown += OnDown;
        slider.PreviewMouseMove += OnMove;
        slider.PreviewMouseLeftButtonUp += OnUp;
        slider.LostMouseCapture += (_, _) => End();
    }

    /// <summary>Raised on the press, before the first value it sets — so a player can pause first.</summary>
    public event EventHandler? Started;

    /// <summary>Raised once the button is up or the capture was lost, after the last value.</summary>
    public event EventHandler? Completed;

    public bool IsScrubbing { get; private set; }

    private void OnDown(object sender, MouseButtonEventArgs e)
    {
        _track ??= _slider.Template?.FindName("PART_Track", _slider) as Track;
        if (_track is null || !_slider.IsEnabled) return;

        e.Handled = true;
        _slider.Focus();
        IsScrubbing = true;
        Started?.Invoke(this, EventArgs.Empty);
        if (!_slider.CaptureMouse())
        {
            End();
            return;
        }
        SetFromPoint(e);
    }

    private void OnMove(object sender, MouseEventArgs e)
    {
        if (IsScrubbing && e.LeftButton == MouseButtonState.Pressed) SetFromPoint(e);
    }

    private void OnUp(object sender, MouseButtonEventArgs e)
    {
        if (!IsScrubbing) return;
        e.Handled = true;
        SetFromPoint(e);
        _slider.ReleaseMouseCapture(); // LostMouseCapture ends the scrub
        End();
    }

    private void End()
    {
        if (!IsScrubbing) return;
        IsScrubbing = false;
        Completed?.Invoke(this, EventArgs.Empty);
    }

    private void SetFromPoint(MouseEventArgs e)
    {
        if (_track is null) return;
        var value = _track.ValueFromPoint(e.GetPosition(_track));
        if (double.IsNaN(value)) return;
        _slider.Value = Math.Clamp(value, _slider.Minimum, _slider.Maximum);
    }
}
