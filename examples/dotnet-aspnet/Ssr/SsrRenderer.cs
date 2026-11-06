// Wraps the Stencil `ssr-wasm` Extism plugin for ASP.NET Core.
//
// The plugin is compiled once per process (CompiledPlugin) and instantiated a fixed number of
// times into a pool. An instance isn't safe for concurrent use, so a request takes one out of
// the pool for as long as it needs it, through an SsrHandle:
//
//     await using var h = renderer.CreateHandle(context);
//     var html = await h.RenderAsync(page);                    // whole document
//     var frag = await h.RenderAsync(markup, SsrOptions.Fragment); // {"fullDocument": false}
//
// Hydration ids: full-document renders always number them from 1. Fragments share one id
// sequence in the plugin, so the fragments of a page never collide; it keeps counting across
// requests unless reset. The handle resets it once, before its first fragment and never between
// fragments, so the same page renders the same way every time.

using System.Diagnostics;
using System.Text.Json;
using System.Threading.Channels;
using Extism.Sdk;

namespace SsrWasm.Ssr;

/// <summary>Stencil render options (see the `ssr` output target docs), sent to the plugin as JSON.
/// Options that are functions, such as beforeSsr, can't be passed to the plugin.</summary>
public static class SsrOptions
{
    /// <summary>Default options: render a whole document.</summary>
    public static readonly IReadOnlyDictionary<string, object?> Document = new Dictionary<string, object?>();

    /// <summary>Render only the components in the input, not a whole document.</summary>
    public static readonly IReadOnlyDictionary<string, object?> Fragment = new Dictionary<string, object?> { ["fullDocument"] = false };
}

public sealed class SsrConfig
{
    /// <summary>Path to dist/ssr-wasm/index.wasm.</summary>
    public required string WasmPath { get; init; }

    /// <summary>Number of plugin instances. Each holds its own QuickJS heap. Renders are pure CPU
    /// work, so more instances than cores only adds memory.</summary>
    public int PoolSize { get; init; } = Environment.ProcessorCount;

    /// <summary>Upper bound for a single plugin call (the manifest's timeout).</summary>
    public TimeSpan CallTimeout { get; init; } = TimeSpan.FromSeconds(30);
}

/// <summary>Owns the compiled plugin and the pool of instances. Register as a singleton.</summary>
public sealed class SsrRenderer : IDisposable
{
    private readonly CompiledPlugin _compiled;
    // A bounded channel is the pool: FIFO, waiting is async (a queued request holds no thread),
    // ReadAsync takes the request's CancellationToken, and the capacity caps memory.
    private readonly Channel<Plugin> _pool;
    private readonly ILogger<SsrRenderer> _log;
    private int _disposed;

    public TimeSpan CompileTime { get; }
    public IReadOnlyList<TimeSpan> InstantiateTimes { get; }
    public int PoolSize { get; }

    public SsrRenderer(SsrConfig config, ILogger<SsrRenderer> log)
    {
        _log = log;
        PoolSize = Math.Max(1, config.PoolSize);
        _pool = Channel.CreateBounded<Plugin>(new BoundedChannelOptions(PoolSize)
        {
            SingleReader = false,
            SingleWriter = false,
        });

        var manifest = new Manifest(new PathWasmSource(config.WasmPath, "main"))
        {
            Timeout = config.CallTimeout,
        };

        var sw = Stopwatch.StartNew();
        _compiled = new CompiledPlugin(manifest, [], withWasi: true);
        CompileTime = sw.Elapsed;

        var times = new List<TimeSpan>(PoolSize);
        for (var i = 0; i < PoolSize; i++)
        {
            sw.Restart();
            var plugin = NewInstance();
            times.Add(sw.Elapsed);
            _pool.Writer.TryWrite(plugin);
        }
        InstantiateTimes = times;
    }

    private Plugin NewInstance()
    {
        var plugin = _compiled.Instantiate();
        try
        {
            // The first call into an extism-js plugin starts QuickJS and evaluates the bundled
            // components. Pay that here rather than on someone's request.
            plugin.Call("resetSsrDocData", ReadOnlySpan<byte>.Empty);
            return plugin;
        }
        catch
        {
            plugin.Dispose();
            throw;
        }
    }

    /// <summary>A handle for one request. It takes an instance from the pool on its first render
    /// and gives it back when disposed. Not thread-safe: use it from one request only.</summary>
    public SsrHandle CreateHandle(HttpContext? context = null) => new(this, context, _log);

    internal ValueTask<Plugin> AcquireAsync(CancellationToken ct) => _pool.Reader.ReadAsync(ct);

    internal void Return(Plugin plugin)
    {
        if (Volatile.Read(ref _disposed) != 0 || !_pool.Writer.TryWrite(plugin))
        {
            plugin.Dispose();
        }
    }

    /// <summary>Swaps a broken instance (failed or cancelled call) for a fresh one, in the
    /// background so the request that broke it isn't held up.</summary>
    internal void Replace(Plugin broken)
    {
        broken.Dispose();
        _ = Task.Run(() =>
        {
            try
            {
                Return(NewInstance());
            }
            catch (Exception e)
            {
                // Leaves the pool one smaller; with none left, requests wait until they're cancelled.
                _log.LogError(e, "ssr: replacing a plugin instance failed");
            }
        });
    }

    public void Dispose()
    {
        if (Interlocked.Exchange(ref _disposed, 1) != 0) return;
        _pool.Writer.TryComplete();
        while (_pool.Reader.TryRead(out var plugin)) plugin.Dispose();
        _compiled.Dispose();
    }

    internal static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web);

    /// <summary>Extracts the HTML from renderToString's JSON result, or throws with its __error.</summary>
    internal static string ParseResult(ReadOnlySpan<byte> json)
    {
        var reader = new Utf8JsonReader(json);
        using var doc = JsonDocument.ParseValue(ref reader);
        var root = doc.RootElement;
        if (root.ValueKind != JsonValueKind.Object)
            throw new SsrException("renderToString returned something other than a JSON object");
        if (root.TryGetProperty("__error", out var error) && error.ValueKind != JsonValueKind.Null)
            throw new SsrException("__error: " + (error.ValueKind == JsonValueKind.String ? error.GetString() : error.GetRawText()));
        if (!root.TryGetProperty("html", out var html) || html.ValueKind != JsonValueKind.String)
            throw new SsrException("renderToString result has no html");
        return html.GetString()!;
    }
}

/// <summary>renderToString's input: <c>{"html": "...", "options": {...}}</c>.</summary>
internal sealed record RenderInput(string Html, IReadOnlyDictionary<string, object?> Options);

public sealed class SsrException(string message) : Exception(message);
