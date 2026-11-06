"""
``{% stencil %}…{% endstencil %}`` - server-render the enclosed component markup as a fragment.

    {% load stencil %}
    {% stencil %}
      <demo-task-list tasks="{{ tasks_json }}"></demo-task-list>
    {% endstencil %}

The block's contents are rendered by the template engine first (so variables are autoescaped as
usual), then passed to ``renderToString`` with ``{"fullDocument": false}``. All blocks in one
request share the request's plugin instance and one hydration-ID sequence (reset before the
first block, never between blocks), so hydration IDs stay unique across the page. Using the tag also tells
``StencilSSRMiddleware`` not to render the response again as a full document.
"""

from django import template
from django.core.exceptions import ImproperlyConfigured
from django.utils.safestring import mark_safe

from ..ssr import for_request

register = template.Library()


@register.tag("stencil")
def do_stencil(parser, token):
    if len(token.split_contents()) != 1:
        raise template.TemplateSyntaxError("{% stencil %} takes no arguments")
    nodelist = parser.parse(("endstencil",))
    parser.delete_first_token()
    return StencilNode(nodelist)


class StencilNode(template.Node):
    def __init__(self, nodelist):
        self.nodelist = nodelist

    def render(self, context):
        request = getattr(context, "request", None)
        if request is None:
            raise ImproperlyConfigured(
                "{% stencil %} needs the request in its context (use render(request, ...)) "
                "so the plugin can be reset once per request."
            )
        html = self.nodelist.render(context)
        return mark_safe(for_request(request).render_fragment(html))
