module StencilSsr
  # Middleware mode: passes every text/html response body through renderToString as a whole
  # document. Also returns the request's plugin instance to the pool once the response is built,
  # whichever mode used it.
  class Middleware
    def initialize(app)
      @app = app
    end

    def call(env)
      status, headers, body = @app.call(env)

      if renderable?(env, status, headers) && !env[ENV_KEY]&.fragments?
        html = read(body)
        body = [StencilSsr.session(env).render_document(html)]
        headers.delete("content-length")
        headers["content-length"] = body.first.bytesize.to_s
      end

      [status, headers, body]
    ensure
      env[ENV_KEY]&.release
    end

    private

    def renderable?(env, status, headers)
      env["REQUEST_METHOD"] != "HEAD" &&
        status >= 200 && ![204, 304].include?(status) &&
        headers["content-type"].to_s.start_with?("text/html") &&
        !headers.key?("content-encoding")
    end

    def read(body)
      html = +""
      body.each { |chunk| html << chunk }
      html.force_encoding(Encoding::UTF_8)
    ensure
      body.close if body.respond_to?(:close)
    end
  end
end
