# Configure the SSR plugin pool. Nothing is compiled or instantiated here: config/puma.rb warms
# the plugin up once the serving process is running (after fork, in cluster mode), so forked
# workers never inherit or share an instance.
StencilSsr.configure(
  wasm_path: Rails.configuration.x.stencil_ssr.wasm_path,
  pool_size: Rails.configuration.x.stencil_ssr.pool_size,
  logger: Rails.logger,
)
