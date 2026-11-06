namespace SsrWasm.Tests;

/// <summary>Points SSR_WASM_PATH / STENCIL_CLIENT_DIR at packages/components/dist unless they're set
/// (the Dockerfile's test stage sets them).</summary>
internal static class Paths
{
    public static readonly string WasmPath = Resolve("SSR_WASM_PATH", "packages/components/dist/ssr-wasm/index.wasm");
    public static readonly string ClientDir = Resolve("STENCIL_CLIENT_DIR", "packages/components/dist/loader-bundle/demo-components");

    private static string Resolve(string env, string repoRelative)
    {
        if (Environment.GetEnvironmentVariable(env) is { Length: > 0 } set) return set;
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir is not null; dir = dir.Parent)
        {
            var candidate = Path.Combine(dir.FullName, repoRelative);
            if (File.Exists(candidate) || Directory.Exists(candidate))
            {
                Environment.SetEnvironmentVariable(env, candidate);
                return candidate;
            }
        }
        throw new InvalidOperationException($"{repoRelative} not found above {AppContext.BaseDirectory}; set {env}");
    }
}
