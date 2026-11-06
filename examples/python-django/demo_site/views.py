import json

from django.http import HttpResponse
from django.shortcuts import render

FRAMEWORK = "Django"

# Deliberately includes <, >, & and quotes to exercise escaping (see CONTRACT.md).
TASKS = [
    {"title": "Write components", "done": True},
    {"title": 'Render with <wasm> & "friends"', "done": False},
    {"title": "Hydrate on the client"},
]


def _context():
    # JSON-encoded here; the template's autoescaping makes it safe inside the attribute.
    return {"framework": FRAMEWORK, "tasks_json": json.dumps(TASKS)}


def middleware_mode(request):
    """Plain template. StencilSSRMiddleware renders the whole response afterwards."""
    return render(request, "middleware.html", _context())


def helper_mode(request):
    """Each component goes through {% stencil %}, so the middleware leaves this response alone."""
    return render(request, "helper.html", _context())


def health(request):
    return HttpResponse("ok", content_type="text/plain")
