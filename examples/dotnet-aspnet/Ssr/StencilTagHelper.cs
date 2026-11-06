using Microsoft.AspNetCore.Razor.TagHelpers;

namespace SsrWasm.Ssr;

/// <summary>
/// Helper mode: <c>&lt;stencil&gt;…&lt;/stencil&gt;</c> renders its content - one component's
/// markup, produced and escaped by Razor as usual - with <c>{"fullDocument": false}</c>, and
/// replaces itself with the result. The rest of the page never goes through the plugin.
/// </summary>
/// <example>
/// <code>
/// &lt;stencil&gt;&lt;demo-task-list tasks="@Model.TasksJson"&gt;&lt;/demo-task-list&gt;&lt;/stencil&gt;
/// &lt;stencil&gt;&lt;partial name="_DemoCard" /&gt;&lt;/stencil&gt;
/// </code>
/// </example>
/// <remarks>
/// Every <c>&lt;stencil&gt;</c> in a request uses the request's scoped <see cref="SsrHandle"/>, so
/// they share one plugin instance and one hydration-id sequence (reset once, before the first).
/// </remarks>
[HtmlTargetElement("stencil")]
public sealed class StencilTagHelper(SsrHandle ssr) : TagHelper
{
    public override async Task ProcessAsync(TagHelperContext context, TagHelperOutput output)
    {
        // Runs the child content, including nested tag helpers such as <partial>.
        var content = await output.GetChildContentAsync();
        var markup = content.GetContent().Trim();

        output.TagName = null; // emit only the content, not a <stencil> element
        output.Content.SetHtmlContent(await ssr.RenderAsync(markup, SsrOptions.Fragment));
    }
}
