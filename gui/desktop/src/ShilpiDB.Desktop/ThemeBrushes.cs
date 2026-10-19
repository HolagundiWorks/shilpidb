using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using Windows.UI;

namespace ShilpiDB.Desktop;

/// <summary>
/// Reads WinUI theme-dictionary brushes for the element's current light or dark theme.
/// <see cref="Application.Current"/> resources stay on the startup theme, so a runtime
/// switch has to look in the Light / Default dictionaries directly.
/// </summary>
static class ThemeBrushes
{
    public static Brush Brush(ElementTheme theme, string key)
    {
        var found = Find(theme, key);
        if (found is Brush brush)
            return brush;
        if (found is Color color)
            return new SolidColorBrush(color);
        throw new InvalidOperationException($"WinUI theme resource '{key}' was not found.");
    }

    public static Color Color(ElementTheme theme, string key)
    {
        var found = Find(theme, key);
        return found switch
        {
            Color color => color,
            SolidColorBrush brush => brush.Color,
            _ => throw new InvalidOperationException($"WinUI theme resource '{key}' was not found."),
        };
    }

    private static object? Find(ElementTheme theme, string key)
    {
        foreach (var themeKey in ThemeKeys(theme))
        {
            if (TryFind(Application.Current.Resources, themeKey, key, out var value))
                return value;
        }

        return Application.Current.Resources.TryGetValue(key, out var fallback) ? fallback : null;
    }

    private static IEnumerable<string> ThemeKeys(ElementTheme theme) =>
        theme == ElementTheme.Light ? ["Light"] : ["Default", "Dark"];

    private static bool TryFind(ResourceDictionary dictionary, string themeKey, string key, out object? value)
    {
        if (TryThemeDictionary(dictionary, themeKey, out var theme) && theme.TryGetValue(key, out value))
            return true;

        foreach (var merged in dictionary.MergedDictionaries)
        {
            if (TryFind(merged, themeKey, key, out value))
                return true;
        }

        value = null;
        return false;
    }

    private static bool TryThemeDictionary(ResourceDictionary dictionary, string themeKey, out ResourceDictionary theme)
    {
        if (dictionary.ThemeDictionaries.TryGetValue(themeKey, out var direct) && direct is ResourceDictionary directDictionary)
        {
            theme = directDictionary;
            return true;
        }

        foreach (var pair in dictionary.ThemeDictionaries)
        {
            if (pair.Key is string name
                && name.Equals(themeKey, StringComparison.OrdinalIgnoreCase)
                && pair.Value is ResourceDictionary named)
            {
                theme = named;
                return true;
            }
        }

        theme = null!;
        return false;
    }
}
