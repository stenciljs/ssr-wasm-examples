// Wraps the Stencil `ssr-wasm` Extism plugin: compiles it once per process and keeps a fixed
// pool of instances. An instance isn't safe for concurrent use, so each request borrows one
// through an SsrHandle (in this app, the scoped service registered in Program.cs):
//
//     using var h = renderer.CreateHandle(cancellationToken);
//     var html = await h.RenderAsync(page);                   // whole document
//     var frag = await h.RenderAsync(markup, fragment: true); // {"fullDocument": false}

using System.Text.Json;
using System.Text.Json.Serialization;
using System.Threading.Channels;
using Extism.Sdk;

namespace SsrWasm.Ssr;

/// <summary>Owns the compiled plugin and the pool of instances. Register as a singleton.</summary>
public sealed class SsrRenderer : IDisposable
{
    private readonly CompiledPlugin _compiled;
    // The pool. A bounded channel waits asynchronously (a queued request holds no thread)
    // and honours the request's CancellationToken.
    private readonly Channel<Plugin> _pool;
    private readonly ILogger _log;

    public SsrRenderer(string wasmPath, int poolSize, ILogger<SsrRenderer> log)
    {
        _log = log;
        var manifest = new Manifest(new PathWasmSource(wasmPath, "main"))
        {
            Timeout = TimeSpan.FromSeconds(30), // upper bound for a single call
        };
        _compiled = new CompiledPlugin(manifest, [], withWasi: true);

        _pool = Channel.CreateBounded<Plugin>(poolSize);
        for (var i = 0; i < poolSize; i++)
        {
            _pool.Writer.TryWrite(NewInstance());
        }
    }

    private Plugin NewInstance()
    {
        var plugin = _compiled.Instantiate();
        // The first call starts QuickJS and evaluates the bundled components.
        // Pay for that here rather than in someone's request.
        plugin.Call("resetSsrDocData", ReadOnlySpan<byte>.Empty);
        return plugin;
    }

    /// <summary>A handle for one request. It takes an instance from the pool on its first render
    /// and gives it back when disposed. Not thread-safe: use it from one request only.</summary>
    public SsrHandle CreateHandle(CancellationToken ct = default) => new(this, ct, _log);

    internal ValueTask<Plugin> RentAsync(CancellationToken ct) => _pool.Reader.ReadAsync(ct);

    internal void Return(Plugin plugin)
    {
        // Fails only once the renderer is disposed.
        if (!_pool.Writer.TryWrite(plugin)) plugin.Dispose();
    }

    /// <summary>A failed or cancelled call can leave an instance unusable: swap it for a new one.</summary>
    internal void Replace(Plugin broken)
    {
        broken.Dispose();
        try
        {
            Return(NewInstance());
        }
        catch (Exception e)
        {
            _log.LogError(e, "ssr: replacing a plugin instance failed");
        }
    }

    public void Dispose()
    {
        _pool.Writer.TryComplete();
        while (_pool.Reader.TryRead(out var plugin)) plugin.Dispose();
        _compiled.Dispose();
    }

    private static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web);

    internal sealed record RenderResult(
        string? Html,
        [property: JsonPropertyName("__error")] JsonElement? Error);

    /// <summary>Extracts the HTML from renderToString's JSON result, or throws with its __error.</summary>
    internal static string ParseResult(ReadOnlySpan<byte> json)
    {
        var result = JsonSerializer.Deserialize<RenderResult>(json, JsonOptions);
        if (result?.Error is { } error) throw new InvalidOperationException("__error: " + error);
        return result?.Html ?? throw new InvalidOperationException("renderToString returned no html");
    }
}

/// <summary>
/// One request's use of a plugin instance. Registered as a scoped service, so the middleware and
/// every <c>&lt;stencil&gt;</c> tag helper in a request share it (and its hydration-id sequence).
/// </summary>
public sealed class SsrHandle(SsrRenderer renderer, CancellationToken ct, ILogger log) : IDisposable
{
    private Plugin? _plugin;
    private bool _broken;
    private bool _idsReset;

    /// <summary>True once a fragment has been rendered. The middleware then skips the page,
    /// so its components aren't rendered twice.</summary>
    public bool RenderedFragments { get; private set; }

    /// <summary>
    /// Passes <paramref name="html"/> through renderToString. On any error (an <c>__error</c>
    /// result, a failed or cancelled call) it logs and returns <paramref name="html"/> unchanged,
    /// so the components still render on the client.
    /// </summary>
    public async Task<string> RenderAsync(string html, bool fragment = false)
    {
        if (fragment) RenderedFragments = true;
        if (_broken) return html;
        try
        {
            _plugin ??= await renderer.RentAsync(ct);
            if (fragment && !_idsReset)
            {
                // Once per request, before the first fragment - never between fragments.
                Call("resetSsrDocData", []);
                _idsReset = true;
            }
            var input = JsonSerializer.SerializeToUtf8Bytes(new
            {
                html,
                options = fragment ? (object)new { fullDocument = false } : new { },
            });
            return SsrRenderer.ParseResult(Call("renderToString", input));
        }
        catch (Exception e)
        {
            log.LogError("ssr: render failed, serving unrendered HTML: {Error}", e.Message);
            return html;
        }
    }

    // The returned span points into the plugin's memory; it's only valid until the next call.
    private ReadOnlySpan<byte> Call(string function, ReadOnlySpan<byte> input)
    {
        try
        {
            return _plugin!.Call(function, input, ct);
        }
        catch
        {
            _broken = true; // a trap, timeout or cancellation: don't reuse this instance
            throw;
        }
    }

    /// <summary>Gives the instance back to the pool, or replaces it if a call failed.</summary>
    public void Dispose()
    {
        if (_plugin is null) return;
        if (_broken) renderer.Replace(_plugin);
        else renderer.Return(_plugin);
        _plugin = null;
    }
}
