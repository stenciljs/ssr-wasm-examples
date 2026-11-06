package com.example.stencilssr.ssr;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;

import org.extism.sdk.Plugin;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import tools.jackson.databind.json.JsonMapper;

/**
 * Wraps the Stencil {@code ssr-wasm} Extism plugin for a multi-threaded server.
 *
 * <p>An instance isn't thread-safe, so the renderer keeps a fixed pool of them in an
 * {@link ArrayBlockingQueue}. A request takes one for its whole lifetime:
 *
 * <pre>{@code
 * try (SsrHandle h = renderer.acquire()) {
 *     String page = h.render(html, SsrOptions.DOCUMENT);   // whole document
 *     String frag = h.render(markup, SsrOptions.FRAGMENT); // {"fullDocument": false}
 * }
 * }</pre>
 *
 * <p>Compiling: the Java SDK has no compiled-plugin type, so every {@code new Plugin(...)}
 * hands the module to libextism, which compiles it with wasmtime. wasmtime's on-disk cache
 * ({@code ~/.cache/wasmtime}) is what makes that "compile once": the first instance (or the
 * Docker build, with {@code --warm-cache}) compiles and fills it, later instances load the
 * cached machine code.
 */
public final class SsrRenderer implements AutoCloseable {

    private static final Logger log = LoggerFactory.getLogger(SsrRenderer.class);

    /** Configuration; see {@code application.properties} for the env vars that set it. */
    public record Config(Path wasmPath, int poolSize, Duration acquireTimeout, Duration callTimeout) {
        public Config {
            if (poolSize < 1) throw new IllegalArgumentException("poolSize must be >= 1");
        }
    }

    /** Startup timings, for the log line. */
    public record Stats(Duration readManifest, List<Duration> instantiate) {}

    final JsonMapper json = JsonMapper.builder().build();
    private final Config config;
    private final byte[] manifest;
    private final BlockingQueue<Plugin> pool;
    private final ExecutorService replacer = Executors.newSingleThreadExecutor(r -> {
        Thread t = new Thread(r, "ssr-replace");
        t.setDaemon(true);
        return t;
    });
    private final Stats stats;
    private volatile boolean closed;

    /**
     * Creates {@code poolSize} instances and warms each one up. Blocks until they're all ready,
     * so the first requests don't pay for instantiation.
     */
    public SsrRenderer(Config config) {
        this.config = config;
        long t0 = System.nanoTime();
        if (!Files.isReadable(config.wasmPath())) {
            throw new IllegalStateException("ssr-wasm plugin not found at " + config.wasmPath().toAbsolutePath());
        }
        // The manifest is plain JSON, written here rather than with the SDK's Manifest class
        // because that class can't express timeout_ms. libextism enforces it per call.
        this.manifest = json.writeValueAsString(Map.of(
                "wasm", List.of(Map.of("path", config.wasmPath().toAbsolutePath().toString())),
                "timeout_ms", config.callTimeout().toMillis()))
                .getBytes(StandardCharsets.UTF_8);
        Duration readManifest = Duration.ofNanos(System.nanoTime() - t0);

        this.pool = new ArrayBlockingQueue<>(config.poolSize(), true); // fair: FIFO waiters
        List<Duration> times = new ArrayList<>();
        try {
            for (int i = 0; i < config.poolSize(); i++) {
                long start = System.nanoTime();
                pool.add(newInstance());
                times.add(Duration.ofNanos(System.nanoTime() - start));
            }
        } catch (RuntimeException e) {
            close();
            throw e;
        }
        this.stats = new Stats(readManifest, List.copyOf(times));
    }

    private Plugin newInstance() {
        // withWASI: the plugin needs it. No host functions: the plugin only imports Extism's
        // own (libextism provides them; HTTP is denied because the manifest allows no hosts).
        Plugin p = new Plugin(manifest, true, null);
        try {
            // The first call into an extism-js plugin starts QuickJS and evaluates the bundled
            // components. Pay for that here, not on someone's request.
            p.call("resetSsrDocData", new byte[0]);
        } catch (RuntimeException e) {
            p.close();
            throw e;
        }
        return p;
    }

    /**
     * Takes an instance out of the pool, waiting up to {@code acquireTimeout} for one. The
     * handle must be closed, and must only be used by one thread at a time.
     *
     * @throws SsrUnavailableException if no instance became free in time
     */
    public SsrHandle acquire() {
        Plugin p;
        try {
            p = pool.poll(config.acquireTimeout().toMillis(), TimeUnit.MILLISECONDS);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new SsrUnavailableException("interrupted while waiting for a plugin instance");
        }
        if (p == null) {
            throw new SsrUnavailableException("no plugin instance free after " + config.acquireTimeout());
        }
        return new SsrHandle(this, p);
    }

    /** Called by {@link SsrHandle#close()}. */
    void release(Plugin p, boolean broken) {
        if (closed) {
            p.close();
        } else if (!broken) {
            pool.add(p);
        } else {
            // A failed call (trap, timeout) may have left the instance in a bad state. Swap in a
            // fresh one in the background rather than holding up the request that broke it.
            replacer.execute(() -> {
                p.close();
                try {
                    pool.add(newInstance());
                } catch (RuntimeException e) {
                    // Leaves the pool one smaller; with none left, acquire() times out and
                    // pages are served unrendered.
                    log.error("ssr: replacing a plugin instance failed", e);
                }
            });
        }
    }

    public Config config() {
        return config;
    }

    public Stats stats() {
        return stats;
    }

    /** Instances currently idle in the pool. */
    public int available() {
        return pool.size();
    }

    /** Closes the idle instances. Instances still checked out are closed when released. */
    @Override
    public void close() {
        closed = true;
        replacer.shutdownNow();
        Plugin p;
        while ((p = pool.poll()) != null) {
            p.close();
        }
    }
}
