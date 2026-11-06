module StencilHelper
  # Server-renders the component markup in the block as a fragment and inlines the result:
  #
  #   <%= stencil do %>
  #     <demo-counter start="5" step="2"></demo-counter>
  #   <% end %>
  #
  # Every call in a request uses the same plugin instance and one hydration-id sequence (reset
  # before the first call, never between calls), so ids stay unique across the page. Extra keyword arguments are passed through as
  # render options.
  def stencil(**options, &block)
    html = capture(&block).to_str
    StencilSsr.session(request.env).render_fragment(html, **options).html_safe
  end
end
