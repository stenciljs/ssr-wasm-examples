using System.Diagnostics;
using System.Globalization;
using System.Text.Json;
using System.Threading.Channels;
using Extism.Sdk;

namespace SsrWasm.Ssr;

/// <summary>
/// One request's exclusive use of a plugin instance. Registered as a scoped service, so the
/// middleware and every <c>&lt;stencil&gt;</c> tag helper in a request share it (and its
/// hydration-id sequence).
/// </summary>
/// <remarks>
/// The instance is taken from the pool lazily, on the first render, so resolving a handle for a
/// request that never renders (static files, /health, JSON) costs nothing. It goes back to the
/// pool on <see cref="Release"/> or when the request scope is disposed.
///
/// Plugin calls run synchronously on the request's thread: a render is pure CPU work with
/// nothing to await, and the pool size (default: the CPU count) caps how many thread-pool
/// threads can be busy in the plugin at once. Requests waiting for an instance wait
/// asynchronously, without holding a thread.
/// </remarks>
public sealed class SsrHandle : IDisposable
{
    private readonly SsrRenderer _renderer;
    private readonly HttpContext? _context;
    private readonly ILogger _log;
    private Plugin? _plugin;
    private bool _released;
    private bool _broken;
    private bool _fragmentIdsReset;
    private long _elapsedTicks;

    internal SsrHandle(SsrRenderer renderer, HttpContext? context, ILogger log)
    {
        _renderer = renderer;
        _context = context;
        _log = log;
    }

    /// <summary>True once a fragment has been rendered through this handle. The middleware uses it
    /// to leave pages that used the helper alone (re-rendering them would render those components again).</summary>
    public bool RenderedFragments { get; private set; }

    /// <summary>Time spent inside the plugin through this handle (reset + renders).</summary>
    public TimeSpan Elapsed => Stopwatch.GetElapsedTime(0, _elapsedTicks);

    /// <summary>
    /// Passes <paramref name="html"/> through renderToString and returns the rendered HTML. On any
    /// error (a plugin <c>__error</c>, a failed or cancelled call, no instance) it logs and returns
    /// <paramref name="html"/> unchanged, so the components still render on the client.
    /// </summary>
    public async ValueTask<string> RenderAsync(string html, IReadOnlyDictionary<string, object?>? options = null)
    {
        options ??= SsrOptions.Document;
        var fragment = options.TryGetValue("fullDocument", out var full) && full is false;
        if (fragment) RenderedFragments = true;

        if (_released || _broken) return html;
        var ct = _context?.RequestAborted ?? CancellationToken.None;

        if (_plugin is null)
        {
            try
            {
                _plugin = await _renderer.AcquireAsync(ct);
            }
            catch (Exception e) when (e is OperationCanceledException or ChannelClosedException)
            {
                _log.LogWarning("ssr: no plugin instance ({Reason}), serving unrendered HTML", e.Message);
                _released = true; // don't wait again for the rest of this request's renders
                return html;
            }
            if (_context is { Response.HasStarted: false })
            {
                _context.Response.OnStarting(static state =>
                {
                    var h = (SsrHandle)state;
                    h._context!.Response.Headers["Server-Timing"] =
                        string.Create(CultureInfo.InvariantCulture, $"ssr;desc=\"Stencil SSR WASM\";dur={h.Elapsed.TotalMilliseconds:F2}");
                    return Task.CompletedTask;
                }, this);
            }
        }

        try
        {
            if (fragment && !_fragmentIdsReset)
            {
                // Once per request, before the first fragment - never between fragments.
                Call("resetSsrDocData", [], ct);
                _fragmentIdsReset = true;
            }
            var input = JsonSerializer.SerializeToUtf8Bytes(new RenderInput(html, options), SsrRenderer.JsonOptions);
            return SsrRenderer.ParseResult(Call("renderToString", input, ct));
        }
        catch (SsrException e)
        {
            _log.LogError("ssr: renderToString failed, serving unrendered HTML: {Error}", e.Message);
        }
        catch (Exception e) when (ct.IsCancellationRequested)
        {
            _log.LogWarning("ssr: render cancelled ({Reason})", e.Message);
        }
        catch (Exception e)
        {
            _log.LogError(e, "ssr: plugin call failed, serving unrendered HTML");
        }
        return html;
    }

    private ReadOnlySpan<byte> Call(string function, ReadOnlySpan<byte> input, CancellationToken ct)
    {
        var start = Stopwatch.GetTimestamp();
        try
        {
            // The returned span points into the plugin's memory; it's only valid until the next call.
            return _plugin!.Call(function, input, ct);
        }
        catch
        {
            // A trap, timeout or cancellation: don't hand this instance to another request.
            _broken = true;
            throw;
        }
        finally
        {
            _elapsedTicks += Stopwatch.GetTimestamp() - start;
        }
    }

    /// <summary>Gives the instance back to the pool (or replaces it, if a call failed). Further
    /// renders return their input unchanged. Safe to call more than once.</summary>
    public void Release()
    {
        if (_released) return;
        _released = true;
        if (_plugin is null) return;
        if (_broken) _renderer.Replace(_plugin);
        else _renderer.Return(_plugin);
        _plugin = null;
    }

    public void Dispose() => Release();
}
