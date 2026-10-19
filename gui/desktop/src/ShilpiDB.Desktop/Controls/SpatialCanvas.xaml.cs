using System.Globalization;
using Microsoft.Graphics.Canvas.Geometry;
using Microsoft.Graphics.Canvas.Text;
using Microsoft.Graphics.Canvas.UI.Xaml;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using ShilpiDB.Desktop.Models;
using Windows.UI;

namespace ShilpiDB.Desktop.Controls;

public readonly record struct QueryRect(double MinX, double MinY, double MaxX, double MaxY);

public sealed partial class SpatialCanvas : UserControl
{
    private readonly List<SpatialRecord> _records = [];
    private HashSet<ulong>? _matchIds;
    private ulong? _selectedId;
    private QueryRect? _query;
    private bool _connected;
    private bool _released;

    public SpatialCanvas()
    {
        InitializeComponent();
        Loaded += (_, _) => RefreshTheme();
        ActualThemeChanged += (_, _) => RefreshTheme();
    }

    public void RefreshTheme()
    {
        if (_released)
            return;
        Canvas.ClearColor = ThemeBrushes.Color(ResolvedTheme(), "SolidBackgroundFillColorBase");
        if (IsLoaded)
            Canvas.Invalidate();
    }

    public event EventHandler<ulong>? RecordInvoked;

    public void Show(
        IEnumerable<RecordItem> records,
        ulong? selectedId,
        IReadOnlySet<ulong>? matchIds,
        QueryRect? query,
        bool connected)
    {
        _records.Clear();
        foreach (var record in records)
            _records.Add(new SpatialRecord(record.Id, record.MinX, record.MinY, record.MaxX, record.MaxY));
        _selectedId = selectedId;
        _matchIds = matchIds is null ? null : [.. matchIds];
        _query = query;
        _connected = connected;
        if (IsLoaded && !_released)
            Canvas.Invalidate();
    }

    public void ReleaseCanvas()
    {
        if (_released)
            return;
        _released = true;
        Canvas.RemoveFromVisualTree();
    }

    private void Canvas_Draw(CanvasControl sender, CanvasDrawEventArgs args)
    {
        var width = sender.ActualWidth;
        var height = sender.ActualHeight;
        if (width <= 0 || height <= 0)
            return;

        var theme = ResolvedTheme();
        var accent = ThemeBrushes.Color(theme, "AccentFillColorDefaultBrush");
        var text = ThemeBrushes.Color(theme, "TextFillColorPrimary");
        var secondary = ThemeBrushes.Color(theme, "TextFillColorSecondary");
        var canvas = ThemeBrushes.Color(theme, "SolidBackgroundFillColorBase");
        var session = args.DrawingSession;
        session.Clear(canvas);
        using var labelFormat = new CanvasTextFormat { FontFamily = "Segoe UI", FontSize = 12 };

        if (_records.Count == 0 || !TryFit(width, height, out var transform))
        {
            var message = _connected
                ? "No records yet — insert one from the sidebar."
                : "Connect to a server to browse records.";
            session.DrawText(message, 16, 16, secondary, labelFormat);
            return;
        }

        if (_query is { } query)
        {
            var queryRect = ScreenRect(transform, query.MinX, query.MinY, query.MaxX, query.MaxY);
            using var dashes = new CanvasStrokeStyle { DashStyle = CanvasDashStyle.Dash };
            session.FillRectangle(queryRect.X, queryRect.Y, queryRect.W, queryRect.H, WithAlpha(accent, 28));
            session.DrawRectangle(queryRect.X, queryRect.Y, queryRect.W, queryRect.H, accent, 1.5f, dashes);
        }

        foreach (var record in _records)
        {
            var rect = ScreenRect(transform, record.MinX, record.MinY, record.MaxX, record.MaxY);
            var x = rect.X;
            var y = rect.Y;
            var w = rect.W;
            var h = rect.H;
            var match = _matchIds is null || _matchIds.Contains(record.Id);
            var selected = _selectedId == record.Id;
            session.FillRectangle(x, y, w, h, match ? WithAlpha(accent, 72) : WithAlpha(secondary, 36));
            session.DrawRectangle(
                x,
                y,
                w,
                h,
                selected ? text : match ? accent : secondary,
                selected ? 2 : 1);
            if (w > 36 && h > 16)
            {
                session.DrawText(
                    record.Id.ToString(CultureInfo.CurrentCulture),
                    x + 4,
                    y + 2,
                    text,
                    labelFormat);
            }
        }
    }

    private ElementTheme ResolvedTheme() =>
        ActualTheme == ElementTheme.Light ? ElementTheme.Light : ElementTheme.Dark;

    private static Color WithAlpha(Color color, byte alpha) =>
        Color.FromArgb(alpha, color.R, color.G, color.B);

    private void Canvas_PointerPressed(object sender, PointerRoutedEventArgs e)
    {
        if (_records.Count == 0)
            return;
        var width = Canvas.ActualWidth;
        var height = Canvas.ActualHeight;
        if (!TryFit(width, height, out var transform) || transform.Scale <= 0)
            return;

        var point = e.GetCurrentPoint(Canvas).Position;
        var worldX = transform.MinX + (point.X - transform.Pad) / transform.Scale;
        var worldY = transform.MinY + (transform.Height - transform.Pad - point.Y) / transform.Scale;

        ulong? hit = null;
        var bestArea = double.PositiveInfinity;
        foreach (var record in _records)
        {
            var minX = Math.Min(record.MinX, record.MaxX);
            var maxX = Math.Max(record.MinX, record.MaxX);
            var minY = Math.Min(record.MinY, record.MaxY);
            var maxY = Math.Max(record.MinY, record.MaxY);
            if (worldX < minX || worldX > maxX || worldY < minY || worldY > maxY)
                continue;
            var area = Math.Abs((maxX - minX) * (maxY - minY));
            if (area <= bestArea)
            {
                bestArea = area;
                hit = record.Id;
            }
        }

        if (hit is ulong id)
        {
            e.Handled = true;
            RecordInvoked?.Invoke(this, id);
        }
    }

    private bool TryFit(double width, double height, out ViewTransform transform)
    {
        transform = default;
        if (_records.Count == 0)
            return false;

        var minX = double.PositiveInfinity;
        var minY = double.PositiveInfinity;
        var maxX = double.NegativeInfinity;
        var maxY = double.NegativeInfinity;
        void Include(double x0, double y0, double x1, double y1)
        {
            minX = Math.Min(minX, Math.Min(x0, x1));
            minY = Math.Min(minY, Math.Min(y0, y1));
            maxX = Math.Max(maxX, Math.Max(x0, x1));
            maxY = Math.Max(maxY, Math.Max(y0, y1));
        }

        foreach (var record in _records)
            Include(record.MinX, record.MinY, record.MaxX, record.MaxY);
        if (_query is { } query)
            Include(query.MinX, query.MinY, query.MaxX, query.MaxY);

        var spanX = Math.Max(maxX - minX, 1e-6);
        var spanY = Math.Max(maxY - minY, 1e-6);
        const double pad = 24;
        var scale = Math.Min(Math.Max(width - pad * 2, 1) / spanX, Math.Max(height - pad * 2, 1) / spanY);
        transform = new ViewTransform(minX, minY, scale, pad, height);
        return true;
    }

    private static (float X, float Y, float W, float H) ScreenRect(ViewTransform transform, double minX, double minY, double maxX, double maxY)
    {
        var x0 = transform.Pad + (minX - transform.MinX) * transform.Scale;
        var y0 = transform.Height - transform.Pad - (minY - transform.MinY) * transform.Scale;
        var x1 = transform.Pad + (maxX - transform.MinX) * transform.Scale;
        var y1 = transform.Height - transform.Pad - (maxY - transform.MinY) * transform.Scale;
        return (
            (float)Math.Min(x0, x1),
            (float)Math.Min(y0, y1),
            (float)Math.Max(Math.Abs(x1 - x0), 1),
            (float)Math.Max(Math.Abs(y1 - y0), 1));
    }

    private readonly record struct SpatialRecord(ulong Id, double MinX, double MinY, double MaxX, double MaxY);

    private readonly record struct ViewTransform(double MinX, double MinY, double Scale, double Pad, double Height);
}
