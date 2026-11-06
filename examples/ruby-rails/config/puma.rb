# Each Puma thread handles one request at a time and checks out its own plugin instance for that
# request, so the plugin pool (config/application.rb) is sized to match the thread count.
threads_count = Integer(ENV.fetch("RAILS_MAX_THREADS", 3))
threads threads_count, threads_count

port ENV.fetch("PORT", 8003)

# Workers (processes) give real parallelism: a plugin call holds Ruby's GVL (see README), so
# threads in one process take turns rendering. Each worker builds its own instances after fork.
worker_count = Integer(ENV.fetch("WEB_CONCURRENCY", 0))
if worker_count > 0
  workers worker_count
  preload_app!
  # Compile the plugin in each worker after the fork (never in the master).
  before_worker_boot { StencilSsr.warm_up }
else
  after_booted { StencilSsr.warm_up }
end

pidfile ENV["PIDFILE"] if ENV["PIDFILE"]
