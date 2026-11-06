// ASP.NET Core + Razor Pages serving the ssr-wasm example pages through the Extism .NET SDK.
//
//   GET /          middleware mode: StencilSsrMiddleware renders the whole HTML response
//   GET /helper    helper mode: the page renders each component through <stencil>…</stencil>
//   GET /health    "ok", without touching the plugin
//   GET /stencil/* the client bundle
//
//   dotnet server.dll --warm-cache    compile the plugin (fills wasmtime's cache) and exit
//   dotnet server.dll --healthcheck   GET /health on the local server, exit non-zero on failure

using Microsoft.Extensions.FileProviders;
using SsrWasm.Ssr;

if (args.Contains("--healthcheck"))
{
    return await CheckHealth();
}

var builder = WebApplication.CreateBuilder(args);

// Listen on 8007 unless ASPNETCORE_URLS / ASPNETCORE_HTTP_PORTS (or --urls) say otherwise.
if (builder.Configuration["urls"] is null && builder.Configuration["http_ports"] is null)
{
    builder.WebHost.UseUrls("http://localhost:8007");
}

var warmCache = args.Contains("--warm-cache");
var ssrConfig = new SsrConfig
{
    WasmPath = Env("SSR_WASM_PATH", "../../packages/components/dist/ssr-wasm/index.wasm"),
    PoolSize = warmCache ? 1 : int.TryParse(Environment.GetEnvironmentVariable("SSR_POOL_SIZE"), out var n) && n > 0 ? n : Environment.ProcessorCount,
};
var clientDir = Path.GetFullPath(Env("STENCIL_CLIENT_DIR", "../../packages/components/dist/loader-bundle/demo-components"));

builder.Services.AddRazorPages();
builder.Services.AddHttpContextAccessor();
builder.Services.AddSingleton(ssrConfig);
builder.Services.AddSingleton<SsrRenderer>();
// One handle per request, shared by the middleware and every <stencil> tag helper.
builder.Services.AddScoped(sp => sp.GetRequiredService<SsrRenderer>()
    .CreateHandle(sp.GetRequiredService<IHttpContextAccessor>().HttpContext));

var app = builder.Build();

// Compile and fill the pool before listening, so the first requests don't pay for it.
var renderer = app.Services.GetRequiredService<SsrRenderer>();
app.Logger.LogInformation(
    "ssr-wasm plugin ready: wasm={Wasm} compile={Compile:F0}ms instantiate=[{Instantiate}] pool={Pool}",
    ssrConfig.WasmPath, renderer.CompileTime.TotalMilliseconds,
    string.Join(", ", renderer.InstantiateTimes.Select(t => $"{t.TotalMilliseconds:F0}ms")), renderer.PoolSize);
if (warmCache)
{
    return 0;
}

// The client bundle. Before the SSR middleware, so static files never pass through it.
app.UseStaticFiles(new StaticFileOptions
{
    FileProvider = new PhysicalFileProvider(clientDir),
    RequestPath = "/stencil",
});

app.UseRouting();
app.UseMiddleware<StencilSsrMiddleware>();

app.MapGet("/health", () => "ok");
app.MapRazorPages();

app.Run();
return 0;

static string Env(string key, string fallback) =>
    Environment.GetEnvironmentVariable(key) is { Length: > 0 } v ? v : fallback;

// The container HEALTHCHECK: the chiseled runtime image has no shell or curl.
static async Task<int> CheckHealth()
{
    var port = Environment.GetEnvironmentVariable("ASPNETCORE_HTTP_PORTS")?.Split(',', ';')[0] is { Length: > 0 } p ? p : "8007";
    using var http = new HttpClient { Timeout = TimeSpan.FromSeconds(3) };
    try
    {
        var res = await http.GetAsync($"http://127.0.0.1:{port}/health");
        return res.IsSuccessStatusCode ? 0 : 1;
    }
    catch (Exception e)
    {
        Console.Error.WriteLine(e.Message);
        return 1;
    }
}

/// <summary>Visible to the tests' WebApplicationFactory.</summary>
public partial class Program;
