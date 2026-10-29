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
/// every move sets <see cref="RangeBase.Value"/> from the point under the cursor, so the thumb
/// centres on it. Keyboard seeking is left alone.
/// <para>
/// Not <see cref="Track.ValueFromPoint"/>: that answers <c>Value + distance from the thumb</c>, and
/// the thumb's position is only refreshed by the next arrange — so the first move after the press
/// counted the jump twice, and the thumb overshot the cursor before snapping back. The value here
/// comes from the track's own length alone.
/// </para>
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
        var horizontal = _track.Orientation == Orientation.Horizontal;
        var point = e.GetPosition(_track);
        var length = horizontal ? _track.ActualWidth : _track.ActualHeight;
        var thumb = _track.Thumb is { } t ? (horizontal ? t.ActualWidth : t.ActualHeight) : 0;
        var travel = length - thumb;
        if (travel <= 0) return;

        var fraction = Math.Clamp(((horizontal ? point.X : point.Y) - thumb / 2) / travel, 0, 1);
        // Horizontal runs left-to-right and vertical bottom-up, each unless reversed.
        if (horizontal == _track.IsDirectionReversed) fraction = 1 - fraction;
        _slider.Value = _slider.Minimum + fraction * (_slider.Maximum - _slider.Minimum);
    }
}
