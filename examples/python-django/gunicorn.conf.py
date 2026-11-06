"""gunicorn settings (loaded automatically from the working directory)."""

import os

wsgi_app = "demo_site.wsgi:application"
bind = f"0.0.0.0:{os.environ.get('PORT', '8002')}"

# Threaded workers: each worker process compiles the plugin once, and each of its threads
# gets its own plugin instance (stencil_ssr/ssr.py). workers x threads = max instances.
worker_class = "gthread"
workers = int(os.environ.get("WEB_CONCURRENCY", "2"))
threads = int(os.environ.get("GUNICORN_THREADS", "4"))

accesslog = "-"
# gunicorn 25+ opens a control socket under ~/.gunicorn by default; this demo doesn't use it.
control_socket_disable = True
# No preload_app: the wasm runtime is created after fork, in each worker.


def post_worker_init(worker):
    # Compile the module before the worker takes traffic, so the first request doesn't pay for it.
    from stencil_ssr.ssr import compiled_plugin

    compiled_plugin()
