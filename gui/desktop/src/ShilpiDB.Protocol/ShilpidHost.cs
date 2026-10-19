using System.Diagnostics;
using System.Globalization;
using System.Text;

namespace ShilpiDB.Protocol;

/// <summary>Finds a <c>shilpid</c> binary and starts it on an OS-assigned local port.</summary>
public static class ShilpidLocator
{
    public static string? Find(string? startDirectory = null)
    {
        var fromEnv = Environment.GetEnvironmentVariable("SHILPID_PATH");
        if (IsFile(fromEnv))
            return Path.GetFullPath(fromEnv!);

        startDirectory ??= AppContext.BaseDirectory;
        var beside = Path.Combine(startDirectory, ExeName());
        if (IsFile(beside))
            return Path.GetFullPath(beside);

        var dir = new DirectoryInfo(startDirectory);
        for (var i = 0; i < 12 && dir is not null; i++, dir = dir.Parent)
        {
            foreach (var config in new[] { "debug", "release" })
            {
                var candidate = Path.Combine(dir.FullName, "target", config, ExeName());
                if (IsFile(candidate))
                    return candidate;
            }
        }

        var path = Environment.GetEnvironmentVariable("PATH");
        if (string.IsNullOrEmpty(path))
            return null;

        foreach (var folder in path.Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries))
        {
            var candidate = Path.Combine(folder.Trim(), ExeName());
            if (IsFile(candidate))
                return Path.GetFullPath(candidate);
        }

        return null;
    }

    private static string ExeName() => OperatingSystem.IsWindows() ? "shilpid.exe" : "shilpid";

    private static bool IsFile(string? path)
    {
        if (string.IsNullOrWhiteSpace(path))
            return false;
        try
        {
            return File.Exists(path);
        }
        catch (IOException)
        {
            return false;
        }
        catch (UnauthorizedAccessException)
        {
            return false;
        }
    }
}

public sealed class ShilpidHost : IDisposable
{
    private Process? _process;
    private CancellationTokenSource? _pumpCancel;

    public async Task<string> StartAsync(string dataPath, CancellationToken cancellationToken = default)
    {
        Stop();
        var exe = ShilpidLocator.Find()
            ?? throw new FileNotFoundException(
                "Couldn't find shilpid.exe. Build it with `cargo build -p shilpid`, or set SHILPID_PATH to the executable.");

        var directory = Path.GetDirectoryName(Path.GetFullPath(dataPath))
            ?? throw new ProtocolException("data path has no directory");
        Directory.CreateDirectory(directory);

        var psi = new ProcessStartInfo
        {
            FileName = exe,
            WorkingDirectory = directory,
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardError = true,
            RedirectStandardOutput = true,
            StandardErrorEncoding = Encoding.UTF8,
            StandardOutputEncoding = Encoding.UTF8,
        };
        psi.ArgumentList.Add("--bind");
        psi.ArgumentList.Add("127.0.0.1:0");
        psi.ArgumentList.Add("--data");
        psi.ArgumentList.Add(dataPath);

        var process = new Process { StartInfo = psi, EnableRaisingEvents = true };
        if (!process.Start())
            throw new ProtocolException("failed to start shilpid");

        _process = process;
        _pumpCancel = new CancellationTokenSource();
        var stderr = process.StandardError;
        var stdout = process.StandardOutput;
        var pumpToken = _pumpCancel.Token;
        _ = Task.Run(async () =>
        {
            try
            {
                while (await stdout.ReadLineAsync(pumpToken).ConfigureAwait(false) is not null)
                {
                }
            }
            catch (OperationCanceledException)
            {
            }
            catch (IOException)
            {
            }
        }, pumpToken);

        var address = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
        _ = Task.Run(async () =>
        {
            var tail = new StringBuilder();
            try
            {
                while (true)
                {
                    var line = await stderr.ReadLineAsync(pumpToken).ConfigureAwait(false);
                    if (line is null)
                    {
                        var code = process.HasExited
                            ? process.ExitCode.ToString(CultureInfo.InvariantCulture)
                            : "unknown";
                        var extra = tail.Length == 0 ? "" : " " + tail;
                        address.TrySetException(new IOException(
                            $"shilpid exited before it started listening (code {code}).{extra}"));
                        return;
                    }

                    tail.Append(' ').Append(line);
                    if (tail.Length > 2000)
                        tail.Remove(0, tail.Length - 2000);

                    const string prefix = "shilpid listening on ";
                    var trimmed = line.Trim();
                    if (trimmed.StartsWith(prefix, StringComparison.Ordinal))
                    {
                        address.TrySetResult(trimmed[prefix.Length..].Trim());
                        while (await stderr.ReadLineAsync(pumpToken).ConfigureAwait(false) is not null)
                        {
                        }
                        return;
                    }
                }
            }
            catch (OperationCanceledException)
            {
                address.TrySetCanceled(pumpToken);
            }
            catch (Exception ex)
            {
                address.TrySetException(ex);
            }
        }, pumpToken);

        var completed = await Task.WhenAny(address.Task, Task.Delay(TimeSpan.FromSeconds(5), cancellationToken))
            .ConfigureAwait(false);
        if (completed != address.Task)
        {
            Stop();
            throw new TimeoutException("timed out waiting for shilpid to start");
        }

        try
        {
            return await address.Task.ConfigureAwait(false);
        }
        catch
        {
            Stop();
            throw;
        }
    }

    public void Stop()
    {
        try { _pumpCancel?.Cancel(); } catch (ObjectDisposedException) { }

        var process = _process;
        _process = null;
        if (process is null)
            return;

        try
        {
            if (!process.HasExited)
            {
                process.Kill(entireProcessTree: true);
                process.WaitForExit(2000);
            }
        }
        catch (InvalidOperationException)
        {
        }
        catch (System.ComponentModel.Win32Exception)
        {
        }
        finally
        {
            process.Dispose();
            _pumpCancel?.Dispose();
            _pumpCancel = null;
        }
    }

    public void Dispose() => Stop();
}
