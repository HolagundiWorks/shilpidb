using System.Globalization;
using System.Net.Sockets;
using System.Text;

namespace ShilpiDB.Protocol;

/// <summary>
/// One TCP connection to a <c>shilpid</c> server. Calls are serialized; the
/// desktop window talks to one server at a time.
/// </summary>
public sealed class ShilpiClient : IDisposable
{
    private readonly SemaphoreSlim _gate = new(1, 1);
    private TcpClient? _tcp;
    private NetworkStream? _stream;

    public async Task ConnectAsync(string endpoint, CancellationToken cancellationToken = default)
    {
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            CloseCore();
            var (host, port) = EndpointAddress.Parse(endpoint);
            var tcp = new TcpClient { NoDelay = true };
            using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            timeout.CancelAfter(TimeSpan.FromSeconds(5));
            var started = Environment.TickCount64;
            while (true)
            {
                try
                {
                    await tcp.ConnectAsync(host, port, timeout.Token).ConfigureAwait(false);
                    break;
                }
                catch (SocketException) when (Environment.TickCount64 - started < 1000)
                {
                    tcp.Dispose();
                    tcp = new TcpClient { NoDelay = true };
                    await Task.Delay(20, timeout.Token).ConfigureAwait(false);
                }
            }

            tcp.ReceiveTimeout = 15_000;
            tcp.SendTimeout = 15_000;
            _tcp = tcp;
            _stream = tcp.GetStream();
            var response = Transact(new ShilpiRequest.Ping());
            if (response is not ShilpiResponse.Pong)
            {
                CloseCore();
                throw new ProtocolException($"unexpected reply to ping: {response.GetType().Name}");
            }
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            CloseCore();
            throw new TimeoutException($"timed out connecting to {endpoint.Trim()}");
        }
        catch
        {
            CloseCore();
            throw;
        }
        finally
        {
            _gate.Release();
        }
    }

    public Task DisconnectAsync() => Task.Run(Disconnect);

    public void Disconnect()
    {
        _gate.Wait();
        try
        {
            CloseCore();
        }
        finally
        {
            _gate.Release();
        }
    }

    public Task<IReadOnlyList<RecordBox>> ListBoxesAsync(CancellationToken cancellationToken = default) =>
        Task.Run(() => WithConnection(() =>
            Transact(new ShilpiRequest.ListBoxes()) switch
            {
                ShilpiResponse.Records records => records.Boxes,
                var other => throw Unexpected(other),
            }), cancellationToken);

    public Task<RecordDetail?> GetAsync(ulong id, CancellationToken cancellationToken = default) =>
        Task.Run(() => WithConnection(() =>
            Transact(new ShilpiRequest.Get(id)) switch
            {
                ShilpiResponse.Found found => ToDetail(id, found),
                ShilpiResponse.NotFound => null,
                var other => throw Unexpected(other),
            }), cancellationToken);

    public Task InsertAsync(ulong id, double minX, double minY, double maxX, double maxY, byte[] payload, CancellationToken cancellationToken = default) =>
        Task.Run(() => WithConnection(() =>
        {
            ExpectOk(Transact(new ShilpiRequest.Insert(id, minX, minY, maxX, maxY, payload)));
            return true;
        }), cancellationToken);

    public Task<bool> DeleteAsync(ulong id, CancellationToken cancellationToken = default) =>
        Task.Run(() => WithConnection(() =>
            Transact(new ShilpiRequest.Delete(id)) switch
            {
                ShilpiResponse.Deleted deleted => deleted.Existed,
                var other => throw Unexpected(other),
            }), cancellationToken);

    public Task<IReadOnlyList<ulong>> QueryBboxAsync(double minX, double minY, double maxX, double maxY, CancellationToken cancellationToken = default) =>
        Task.Run(() => WithConnection(() =>
            Transact(new ShilpiRequest.QueryBbox(minX, minY, maxX, maxY)) switch
            {
                ShilpiResponse.Ids ids => ids.Values,
                var other => throw Unexpected(other),
            }), cancellationToken);

    public Task<DatabaseStats> StatsAsync(CancellationToken cancellationToken = default) =>
        Task.Run(() => WithConnection(() =>
            Transact(new ShilpiRequest.Stats()) switch
            {
                ShilpiResponse.Stats stats => new DatabaseStats(stats.Count, stats.CellSize),
                var other => throw Unexpected(other),
            }), cancellationToken);

    public Task SaveAsync(CancellationToken cancellationToken = default) =>
        Task.Run(() => WithConnection(() =>
        {
            ExpectOk(Transact(new ShilpiRequest.Save()));
            return true;
        }), cancellationToken);

    public void Dispose()
    {
        Disconnect();
        _gate.Dispose();
    }

    private T WithConnection<T>(Func<T> action)
    {
        _gate.Wait();
        try
        {
            if (_stream is null)
                throw new ProtocolException("not connected to a shilpid server");
            return action();
        }
        finally
        {
            _gate.Release();
        }
    }

    private ShilpiResponse Transact(ShilpiRequest request)
    {
        var stream = _stream ?? throw new ProtocolException("not connected to a shilpid server");
        try
        {
            Frame.Write(stream, WireFormat.EncodeRequest(request));
            var body = Frame.Read(stream) ?? throw new IOException("connection closed by server");
            return WireFormat.DecodeResponse(body);
        }
        catch (Exception ex) when (ex is IOException or SocketException or ObjectDisposedException or EndOfStreamException or ProtocolException)
        {
            CloseCore();
            throw;
        }
    }

    private static void ExpectOk(ShilpiResponse response)
    {
        switch (response)
        {
            case ShilpiResponse.Ok:
                return;
            case ShilpiResponse.Err err:
                throw new ProtocolException(err.Message);
            default:
                throw Unexpected(response);
        }
    }

    private static ProtocolException Unexpected(ShilpiResponse response) =>
        new($"unexpected reply: {response.GetType().Name}");

    private static RecordDetail ToDetail(ulong id, ShilpiResponse.Found found)
    {
        var text = Encoding.UTF8.GetString(found.Payload);
        var hex = Convert.ToHexString(found.Payload).ToLowerInvariant();
        return new RecordDetail(id, found.MinX, found.MinY, found.MaxX, found.MaxY, text, hex);
    }

    private void CloseCore()
    {
        try { _stream?.Dispose(); } catch (IOException) { }
        try { _tcp?.Dispose(); } catch (IOException) { }
        _stream = null;
        _tcp = null;
    }
}

public static class EndpointAddress
{
    public static (string Host, int Port) Parse(string text)
    {
        if (string.IsNullOrWhiteSpace(text))
            throw new ProtocolException("expected host:port");

        text = text.Trim();
        var idx = text.LastIndexOf(':');
        if (idx <= 0
            || idx == text.Length - 1
            || !int.TryParse(text[(idx + 1)..], NumberStyles.None, CultureInfo.InvariantCulture, out var port)
            || port is < 1 or > 65535)
        {
            throw new ProtocolException($"expected host:port, got '{text}'");
        }

        var host = text[..idx];
        if (host.Length >= 2 && host[0] == '[' && host[^1] == ']')
            host = host[1..^1];
        return (host, port);
    }
}
