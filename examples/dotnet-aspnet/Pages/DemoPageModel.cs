using System.Text.Encodings.Web;
using System.Text.Json;
using Microsoft.AspNetCore.Mvc.RazorPages;

namespace SsrWasm.Pages;

/// <summary>The model both pages (/ and /helper) share: the same content, rendered two ways.</summary>
public class DemoPageModel : PageModel
{
    public const string FrameworkName = "ASP.NET Core";

    private record DemoTask(string Title, bool? Done = null);

    private static readonly DemoTask[] Tasks =
    [
        new("Write components", true),
        new("Render with <wasm> & \"friends\"", false),
        new("Hydrate on the client"), // no "done" key at all
    ];

    // System.Text.Json escapes <, >, & and quotes as < etc. by default, as if the JSON were
    // going into a <script>. Here HTML escaping is Razor's job (it attribute-encodes the string
    // when it's written into tasks="..."), so the JSON itself is left plain.
    private static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web)
    {
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        DefaultIgnoreCondition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull,
    };

    public string Framework => FrameworkName;

    /// <summary>The task list as a JSON string, for demo-task-list's `tasks` attribute.</summary>
    public string TasksJson { get; } = JsonSerializer.Serialize(Tasks, JsonOptions);
}
