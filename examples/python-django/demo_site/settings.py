"""A deliberately small Django config: no database, no auth/sessions/admin, one reusable app."""

import os
from pathlib import Path

BASE_DIR = Path(__file__).resolve().parent.parent
REPO_ROOT = BASE_DIR.parent.parent

DEBUG = os.environ.get("DJANGO_DEBUG") == "1"
# Nothing here is signed (no sessions, no CSRF), but Django refuses to start without a key.
SECRET_KEY = os.environ.get("DJANGO_SECRET_KEY", "insecure-demo-key-not-used-for-anything")
ALLOWED_HOSTS = os.environ.get("DJANGO_ALLOWED_HOSTS", "*").split(",")

# ---- Stencil SSR WASM inputs (see CONTRACT.md) ----
SSR_WASM_PATH = Path(
    os.environ.get("SSR_WASM_PATH", REPO_ROOT / "packages/components/dist/ssr-wasm/index.wasm")
)
STENCIL_CLIENT_DIR = Path(
    os.environ.get(
        "STENCIL_CLIENT_DIR", REPO_ROOT / "packages/components/dist/loader-bundle/demo-components"
    )
)

INSTALLED_APPS = [
    "stencil_ssr",  # plugin wrapper, middleware and {% stencil %} tag
]

MIDDLEWARE = [
    # Serves the client bundle at /stencil/ straight from STENCIL_CLIENT_DIR (works with DEBUG off).
    "whitenoise.middleware.WhiteNoiseMiddleware",
    # Middleware mode: post-processes whole text/html responses through renderToString.
    "stencil_ssr.middleware.StencilSSRMiddleware",
]

ROOT_URLCONF = "demo_site.urls"
WSGI_APPLICATION = "demo_site.wsgi.application"

TEMPLATES = [
    {
        "BACKEND": "django.template.backends.django.DjangoTemplates",
        "DIRS": [BASE_DIR / "demo_site" / "templates"],
        "APP_DIRS": True,
        "OPTIONS": {"context_processors": []},
    }
]

DATABASES = {}

# The client bundle is the only static content. WhiteNoise serves STATIC_ROOT at STATIC_URL, so
# pointing STATIC_ROOT at the bundle directory needs no collectstatic step, and `{% static %}`
# produces /stencil/... URLs.
STATIC_URL = "/stencil/"
STATIC_ROOT = STENCIL_CLIENT_DIR

USE_I18N = False
USE_TZ = True
TIME_ZONE = "UTC"

LOGGING = {
    "version": 1,
    "disable_existing_loggers": False,
    "formatters": {"plain": {"format": "%(asctime)s %(levelname)s %(name)s [%(threadName)s] %(message)s"}},
    "handlers": {"console": {"class": "logging.StreamHandler", "formatter": "plain"}},
    "loggers": {
        "stencil_ssr": {"handlers": ["console"], "level": os.environ.get("SSR_LOG_LEVEL", "INFO")},
    },
}
