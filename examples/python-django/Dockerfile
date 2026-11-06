# Build from the repo root:
#   docker build -f examples/python-django/Dockerfile -t ssr-wasm-python-django .
FROM python:3.13-slim

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1

# Extism enables wasmtime's compilation cache, which lives under $HOME/.cache, so the app user
# needs a writable home (running as `nobody` fails with "Permission denied").
RUN useradd --create-home --uid 10001 app
WORKDIR /app

# 1. dependencies (changes least often)
COPY examples/python-django/requirements.txt ./
RUN pip install -r requirements.txt

# 2. the Stencil build artifacts
COPY packages/components/dist/ssr-wasm/index.wasm /srv/stencil/ssr-wasm/index.wasm
COPY packages/components/dist/loader-bundle/demo-components/ /srv/stencil/client/
ENV SSR_WASM_PATH=/srv/stencil/ssr-wasm/index.wasm \
    STENCIL_CLIENT_DIR=/srv/stencil/client

# 3. the app (changes most often)
COPY examples/python-django/manage.py examples/python-django/gunicorn.conf.py ./
COPY examples/python-django/demo_site/ ./demo_site/
COPY examples/python-django/stencil_ssr/ ./stencil_ssr/

USER app
# Compile the plugin once at build time so wasmtime's cache is warm and workers start fast.
RUN python -c "import extism.extism as e, os; e.CompiledPlugin(open(os.environ['SSR_WASM_PATH'], 'rb').read(), wasi=True)"
EXPOSE 8002
CMD ["gunicorn"]
