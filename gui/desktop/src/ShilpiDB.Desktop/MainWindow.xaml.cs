using System.Collections.ObjectModel;
using System.Globalization;
using System.Net.Sockets;
using System.Text;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using ShilpiDB.Desktop.Controls;
using ShilpiDB.Desktop.Models;
using ShilpiDB.Protocol;
using Windows.Graphics;
using Windows.System;

namespace ShilpiDB.Desktop;

public sealed partial class MainWindow : Window
{
    private readonly ShilpiClient _client = new();
    private readonly ShilpidHost _server = new();
    private HashSet<ulong>? _matchIds;
    private QueryRect? _query;
    private ulong? _selectedId;
    private bool _connected;
    private bool _busy;
    private bool _sized;
    private string _statusText = "disconnected";
    private string _statusKind = "disconnected";

    public ObservableCollection<RecordItem> Records { get; } = [];

    public MainWindow()
    {
        InitializeComponent();
        Root.DataContext = this;
        ConfigureWindow();
        if (LoadSavedTheme() is { } saved)
            Root.RequestedTheme = saved;
        Root.ActualThemeChanged += (_, _) => OnThemeChanged();
        Root.Loaded += (_, _) => OnThemeChanged();
        Spatial.RecordInvoked += (_, id) => _ = SelectRecordAsync(id);
        Closed += OnClosed;
        ApplyDisconnected();
        UpdateEnabled();
        OnThemeChanged();
    }

    private void ThemeButton_Click(object sender, RoutedEventArgs e)
    {
        var next = Root.ActualTheme == ElementTheme.Dark ? ElementTheme.Light : ElementTheme.Dark;
        Root.RequestedTheme = next;
        SaveTheme(next);
    }

    private void OnThemeChanged()
    {
        var theme = Root.ActualTheme == ElementTheme.Light ? ElementTheme.Light : ElementTheme.Dark;
        var dark = theme == ElementTheme.Dark;
        ThemeButton.Content = dark ? "Dark" : "Light";
        ToolTipService.SetToolTip(ThemeButton, dark ? "Switch to light theme" : "Switch to dark theme");
        RecordItem.ApplyTheme(theme);
        ApplyCaptionColors(theme);
        ApplyRowStyles();
        Spatial.RefreshTheme();
        SetStatus(_statusText, _statusKind);
    }

    private void ConfigureWindow()
    {
        var presenter = OverlappedPresenter.Create();
        presenter.PreferredMinimumWidth = 1080;
        presenter.PreferredMinimumHeight = 640;
        AppWindow.SetPresenter(presenter);

        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);

        AppWindow.TitleBar.PreferredHeightOption = TitleBarHeightOption.Tall;

        Activated += (_, _) =>
        {
            if (!_sized && AppTitleBar.XamlRoot is { } root)
            {
                _sized = true;
                var scale = root.RasterizationScale <= 0 ? 1 : root.RasterizationScale;
                AppWindow.Resize(new SizeInt32((int)(1200 * scale), (int)(780 * scale)));
            }

            ApplyCaptionInset();
        };
        AppTitleBar.SizeChanged += (_, _) => ApplyCaptionInset();
    }

    private void ApplyCaptionInset()
    {
        if (AppTitleBar.XamlRoot is null)
            return;
        var scale = AppTitleBar.XamlRoot.RasterizationScale;
        if (scale <= 0)
            scale = 1;
        CaptionSpacer.Width = new GridLength(Math.Max(0, AppWindow.TitleBar.RightInset / scale));
    }

    private void ApplyCaptionColors(ElementTheme theme)
    {
        var bar = AppWindow.TitleBar;
        var foreground = ThemeBrushes.Color(theme, "TextFillColorPrimary");
        var secondary = ThemeBrushes.Color(theme, "TextFillColorSecondary");
        var hover = ThemeBrushes.Color(theme, "SubtleFillColorSecondary");
        var pressed = ThemeBrushes.Color(theme, "ControlFillColorSecondary");
        var transparent = Windows.UI.Color.FromArgb(0, 0, 0, 0);
        bar.ForegroundColor = foreground;
        bar.ButtonForegroundColor = foreground;
        bar.ButtonHoverForegroundColor = foreground;
        bar.ButtonPressedForegroundColor = foreground;
        bar.ButtonInactiveForegroundColor = secondary;
        bar.ButtonBackgroundColor = transparent;
        bar.ButtonInactiveBackgroundColor = transparent;
        bar.ButtonHoverBackgroundColor = hover;
        bar.ButtonPressedBackgroundColor = pressed;
    }

    private async void Connect_Click(object sender, RoutedEventArgs e) =>
        await RunAsync(() => ConnectCoreAsync(HostBox.Text));

    private async void Disconnect_Click(object sender, RoutedEventArgs e) =>
        await RunAsync(async () =>
        {
            await _client.DisconnectAsync();
            ApplyDisconnected();
        });

    private async void StartLocal_Click(object sender, RoutedEventArgs e) =>
        await RunAsync(async () =>
        {
            SetStatus("starting local shilpid…", "disconnected");
            await _client.DisconnectAsync();
            ApplyDisconnected();
            var address = await _server.StartAsync(DesktopDataPath());
            HostBox.Text = address;
            await ConnectCoreAsync(address);
        });

    private void HostBox_KeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == VirtualKey.Enter && ConnectButton.IsEnabled)
        {
            e.Handled = true;
            Connect_Click(sender, e);
        }
    }

    private async void Query_Click(object sender, RoutedEventArgs e) =>
        await RunAsync(async () =>
        {
            var minX = QueryMinX.Value;
            var minY = QueryMinY.Value;
            var maxX = QueryMaxX.Value;
            var maxY = QueryMaxY.Value;
            if (!Finite(minX, minY, maxX, maxY))
                throw new ProtocolException("Enter a valid query box.");

            var ids = await _client.QueryBboxAsync(minX, minY, maxX, maxY);
            _matchIds = ids.ToHashSet();
            _query = new QueryRect(minX, minY, maxX, maxY);
            QueryResult.Text = _matchIds.Count == 1 ? "1 match" : $"{_matchIds.Count} matches";
            ApplyRowStyles();
            Redraw();
            ClearNotice();
        });

    private void ClearQuery_Click(object sender, RoutedEventArgs e)
    {
        _matchIds = null;
        _query = null;
        QueryResult.Text = "";
        ApplyRowStyles();
        Redraw();
    }

    private async void Insert_Click(object sender, RoutedEventArgs e) =>
        await RunAsync(async () =>
        {
            if (!Finite(InsertId.Value, InsertMinX.Value, InsertMinY.Value, InsertMaxX.Value, InsertMaxY.Value) || InsertId.Value < 0)
                throw new ProtocolException("Enter a valid id and box.");

            var id = (ulong)InsertId.Value;
            await _client.InsertAsync(
                id,
                InsertMinX.Value,
                InsertMinY.Value,
                InsertMaxX.Value,
                InsertMaxY.Value,
                Encoding.UTF8.GetBytes(InsertPayload.Text ?? ""));
            if (InsertId.Value < double.MaxValue)
                InsertId.Value += 1;
            await RefreshAsync();
            ClearNotice();
        });

    private async void Delete_Click(object sender, RoutedEventArgs e) =>
        await RunAsync(async () =>
        {
            if (_selectedId is not ulong id)
                return;
            var removed = await _client.DeleteAsync(id);
            _selectedId = null;
            ShowDetail(null);
            await RefreshAsync();
            if (!removed)
                ShowNotice(InfoBarSeverity.Warning, "That record was already gone.");
            else
                ClearNotice();
        });

    private async void Save_Click(object sender, RoutedEventArgs e) =>
        await RunAsync(async () =>
        {
            await _client.SaveAsync();
            ShowNotice(InfoBarSeverity.Success, "Saved to disk.");
            if (_connected)
                SetStatus($"connected · {HostBox.Text.Trim()}", "connected");
        });

    private void RecordRow_PointerPressed(object sender, PointerRoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is RecordItem item)
        {
            e.Handled = true;
            _ = SelectRecordAsync(item.Id);
        }
    }

    private void RecordRow_PointerEntered(object sender, PointerRoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is RecordItem item && _selectedId != item.Id)
            item.RowBackground = RecordItem.HoverBackground;
    }

    private void RecordRow_PointerExited(object sender, PointerRoutedEventArgs e) => ApplyRowStyles();

    private async Task ConnectCoreAsync(string endpoint)
    {
        var host = endpoint.Trim();
        if (host.Length == 0)
            throw new ProtocolException("Enter a host:port address.");

        await _client.ConnectAsync(host);
        _connected = true;
        SetStatus($"connected · {host}", "connected");
        ClearNotice();
        await RefreshAsync();
    }

    private async Task RefreshAsync()
    {
        var boxes = await _client.ListBoxesAsync();
        var stats = await _client.StatsAsync();
        var keep = _selectedId;
        Records.Clear();
        foreach (var box in boxes)
            Records.Add(new RecordItem(box.Id, box.MinX, box.MinY, box.MaxX, box.MaxY));

        _selectedId = keep is ulong id && Records.Any(record => record.Id == id) ? id : null;
        if (_selectedId is null)
            ShowDetail(null);

        StatsText.Text = $"{stats.Count.ToString(CultureInfo.CurrentCulture)} record(s) · cell size {stats.CellSize.ToString("0.000", CultureInfo.CurrentCulture)}";
        ApplyRowStyles();
        Redraw();
    }

    private async Task SelectRecordAsync(ulong id)
    {
        if (!_connected || _busy)
            return;

        _selectedId = id;
        ApplyRowStyles();
        Redraw();
        try
        {
            var detail = await _client.GetAsync(id);
            if (_selectedId != id)
                return;
            if (detail is null)
            {
                ShowDetail(null);
                ShowNotice(InfoBarSeverity.Warning, $"Record {id.ToString(CultureInfo.CurrentCulture)} was not found.");
                return;
            }

            ShowDetail(detail);
            ClearNotice();
        }
        catch (Exception ex)
        {
            HandleFault(ex);
        }
    }

    private void ShowDetail(RecordDetail? detail)
    {
        if (detail is null)
        {
            DetailEmpty.Visibility = Visibility.Visible;
            DetailBody.Visibility = Visibility.Collapsed;
            DeleteButton.Visibility = Visibility.Collapsed;
            return;
        }

        DetailEmpty.Visibility = Visibility.Collapsed;
        DetailBody.Visibility = Visibility.Visible;
        DeleteButton.Visibility = Visibility.Visible;
        DetailId.Text = detail.Id.ToString(CultureInfo.CurrentCulture);
        DetailBbox.Text =
            $"({RecordItem.Format(detail.MinX)}, {RecordItem.Format(detail.MinY)}) – ({RecordItem.Format(detail.MaxX)}, {RecordItem.Format(detail.MaxY)})";
        DetailText.Text = string.IsNullOrEmpty(detail.PayloadText) ? "(empty)" : detail.PayloadText;
        DetailHex.Text = string.IsNullOrEmpty(detail.PayloadHex) ? "(empty)" : detail.PayloadHex;
        UpdateEnabled();
    }

    private void ApplyRowStyles()
    {
        foreach (var item in Records)
        {
            var match = _matchIds is null || _matchIds.Contains(item.Id);
            item.RowOpacity = match ? 1 : 0.35;
            item.MatchBarVisibility = _matchIds is not null && match ? Visibility.Visible : Visibility.Collapsed;
            item.RowBackground = item.Id == _selectedId ? RecordItem.SelectedBackground : RecordItem.IdleBackground;
        }
    }

    private void Redraw()
    {
        Spatial.Show(Records, _selectedId, _matchIds, _query, _connected);
        if (!_connected)
        {
            RecordCountText.Text = "Not connected";
            MatchSummary.Text = "";
            return;
        }

        RecordCountText.Text = Records.Count == 1 ? "1 record" : $"{Records.Count} records";
        MatchSummary.Text = _matchIds is null
            ? RecordCountText.Text
            : $"{_matchIds.Count} of {Records.Count} match";
    }

    private void ApplyDisconnected()
    {
        _connected = false;
        _selectedId = null;
        _matchIds = null;
        _query = null;
        QueryResult.Text = "";
        Records.Clear();
        ShowDetail(null);
        StatsText.Text = "Not connected.";
        SetStatus("disconnected", "disconnected");
        Redraw();
    }

    private async Task RunAsync(Func<Task> action)
    {
        if (_busy)
            return;
        _busy = true;
        UpdateEnabled();
        try
        {
            await action();
        }
        catch (Exception ex)
        {
            HandleFault(ex);
        }
        finally
        {
            _busy = false;
            UpdateEnabled();
        }
    }

    private void HandleFault(Exception ex)
    {
        ShowNotice(InfoBarSeverity.Error, ex.Message);
        var transport = ex is IOException or SocketException or EndOfStreamException or TimeoutException
            || ex.InnerException is SocketException;
        if (transport)
        {
            _client.Disconnect();
            ApplyDisconnected();
        }

        SetStatus("error", "error");
    }

    private void UpdateEnabled()
    {
        ConnectButton.IsEnabled = !_busy && !_connected;
        DisconnectButton.IsEnabled = !_busy && _connected;
        StartLocalButton.IsEnabled = !_busy;
        QueryButton.IsEnabled = !_busy && _connected;
        InsertButton.IsEnabled = !_busy && _connected;
        SaveButton.IsEnabled = !_busy && _connected;
        DeleteButton.IsEnabled = !_busy && _connected && _selectedId is not null;
        HostBox.IsEnabled = !_busy;
        BusyRing.IsActive = _busy;
    }

    private void SetStatus(string text, string kind)
    {
        _statusText = text;
        _statusKind = kind;
        StatusText.Text = text.ToUpperInvariant();
        var theme = Root.ActualTheme == ElementTheme.Light ? ElementTheme.Light : ElementTheme.Dark;
        StatusPill.Background = ThemeBrushes.Brush(theme, kind switch
        {
            "connected" => "SystemFillColorSuccessBackgroundBrush",
            "error" => "SystemFillColorCriticalBackgroundBrush",
            _ => "SubtleFillColorSecondaryBrush",
        });
        StatusText.Foreground = ThemeBrushes.Brush(theme, kind switch
        {
            "connected" => "SystemFillColorSuccessBrush",
            "error" => "SystemFillColorCriticalBrush",
            _ => "TextFillColorSecondaryBrush",
        });
    }

    private static ElementTheme? LoadSavedTheme()
    {
        try
        {
            if (!File.Exists(ThemeFile()))
                return null;
            return File.ReadAllText(ThemeFile()).Trim().ToLowerInvariant() switch
            {
                "light" => ElementTheme.Light,
                "dark" => ElementTheme.Dark,
                _ => null,
            };
        }
        catch (IOException)
        {
            return null;
        }
        catch (UnauthorizedAccessException)
        {
            return null;
        }
    }

    private static void SaveTheme(ElementTheme theme)
    {
        try
        {
            var path = ThemeFile();
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            File.WriteAllText(path, theme == ElementTheme.Light ? "light" : "dark");
        }
        catch (IOException)
        {
        }
        catch (UnauthorizedAccessException)
        {
        }
    }

    private static string ThemeFile() =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ShilpiDB", "theme.txt");

    private void ShowNotice(InfoBarSeverity severity, string message)
    {
        ErrorBar.Severity = severity;
        ErrorBar.Message = message;
        ErrorBar.IsOpen = true;
    }

    private void ClearNotice() => ErrorBar.IsOpen = false;

    private static bool Finite(params double[] values) => values.All(double.IsFinite);

    private static string DesktopDataPath()
    {
        var dir = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "ShilpiDB");
        Directory.CreateDirectory(dir);
        return Path.Combine(dir, "desktop.vdb");
    }

    private void OnClosed(object sender, WindowEventArgs args)
    {
        Spatial.ReleaseCanvas();
        _client.Dispose();
        _server.Stop();
    }
}
