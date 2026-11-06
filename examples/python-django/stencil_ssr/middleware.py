from .ssr import REQUEST_ATTR, for_request


class StencilSSRMiddleware:
    """
    Middleware mode: passes every complete ``text/html`` response through ``renderToString``
    (default options, so ``fullDocument: true``).

    Skipped:
      * responses whose template already rendered components with ``{% stencil %}`` (helper
        mode, e.g. ``/helper``) - rendering them again as a full document would re-process
        server-rendered markup;
      * non-HTML, streaming and already-encoded (e.g. gzipped) responses.

    Adds a ``Server-Timing: ssr;dur=<ms>`` header whenever the plugin was used.
    """

    def __init__(self, get_response):
        self.get_response = get_response

    def __call__(self, request):
        response = self.get_response(request)
        renderer = getattr(request, REQUEST_ATTR, None)

        if renderer is None or not renderer.fragments:
            if not self._is_renderable(response):
                return response
            renderer = for_request(request)
            charset = response.charset
            response.content = renderer.render(response.content.decode(charset)).encode(charset)
            if response.has_header("Content-Length"):
                response["Content-Length"] = str(len(response.content))

        response["Server-Timing"] = f"ssr;dur={renderer.seconds * 1000:.1f}"
        return response

    @staticmethod
    def _is_renderable(response) -> bool:
        return (
            not response.streaming
            and response.get("Content-Type", "").startswith("text/html")
            and not response.has_header("Content-Encoding")
            and bool(response.content)
        )
