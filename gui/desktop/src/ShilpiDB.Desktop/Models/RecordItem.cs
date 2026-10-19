using System.ComponentModel;
using System.Globalization;
using System.Runtime.CompilerServices;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using Windows.UI;

namespace ShilpiDB.Desktop.Models;

public sealed class RecordItem : INotifyPropertyChanged
{
    public static readonly SolidColorBrush IdleBackground = new(Colors.Transparent);
    public static readonly SolidColorBrush SelectedBackground = new(Colors.Transparent);
    public static readonly SolidColorBrush HoverBackground = new(Colors.Transparent);

    public static void ApplyTheme(ElementTheme theme)
    {
        var accent = ThemeBrushes.Color(theme, "AccentFillColorDefaultBrush");
        SelectedBackground.Color = Color.FromArgb(0x40, accent.R, accent.G, accent.B);
        HoverBackground.Color = ThemeBrushes.Color(theme, "SubtleFillColorSecondary");
    }

    private double _rowOpacity = 1;
    private Brush _rowBackground = IdleBackground;
    private Visibility _matchBar = Visibility.Collapsed;

    public RecordItem(ulong id, double minX, double minY, double maxX, double maxY)
    {
        Id = id;
        MinX = minX;
        MinY = minY;
        MaxX = maxX;
        MaxY = maxY;
    }

    public ulong Id { get; }
    public double MinX { get; }
    public double MinY { get; }
    public double MaxX { get; }
    public double MaxY { get; }

    public string IdText => Id.ToString(CultureInfo.CurrentCulture);
    public string MinXText => Format(MinX);
    public string MinYText => Format(MinY);
    public string MaxXText => Format(MaxX);
    public string MaxYText => Format(MaxY);

    public double RowOpacity
    {
        get => _rowOpacity;
        set => Set(ref _rowOpacity, value);
    }

    public Brush RowBackground
    {
        get => _rowBackground;
        set => Set(ref _rowBackground, value);
    }

    public Visibility MatchBarVisibility
    {
        get => _matchBar;
        set => Set(ref _matchBar, value);
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    public static string Format(double value) => value.ToString("G15", CultureInfo.CurrentCulture);

    private void Set<T>(ref T field, T value, [CallerMemberName] string? name = null)
    {
        if (EqualityComparer<T>.Default.Equals(field, value))
            return;
        field = value;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
    }
}
